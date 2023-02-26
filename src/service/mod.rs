mod auth;
mod room;

use std::time::Duration;
use std::{collections::HashMap, sync::Arc};

use coin_shuffle_contracts_bindings::utxo::Contract;
use coin_shuffle_core::service::{storage::Storage, waiter::simple::SimpleWaiter, Service as Core};
use coin_shuffle_protos::v1::{
    shuffle_service_server::ShuffleService, ConnectShuffleRoomRequest, IsReadyForShuffleRequest,
    IsReadyForShuffleResponse, JoinShuffleRoomRequest, JoinShuffleRoomResponse, ShuffleEvent,
    ShuffleRoundRequest, ShuffleRoundResponse, SignShuffleTxRequest, SignShuffleTxResponse,
};
use ethers_core::types::U256;
use tokio::sync::Mutex;
use tokio::{sync::mpsc::Sender as StreamSender, time::timeout};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use self::{
    auth::{verify_join_signature, TokensGenerator},
    room::{RoomConnection, RoomEvents},
};

pub struct Service<S, C>
where
    S: Storage,
    C: Contract,
{
    inner: Core<S, SimpleWaiter<S>, C>,
    utxo_contract: C,
    storage: S,
    tokens_generator: TokensGenerator,

    rooms: Arc<Mutex<HashMap<Uuid, StreamSender<RoomEvents>>>>,
}

pub const MIN_ROOM_SIZE: usize = 3; // TODO: Move to config

#[tonic::async_trait]
impl<S, C> ShuffleService for Service<S, C>
where
    S: Storage + 'static,
    C: Contract + Send + Sync + 'static,
{
    async fn join_shuffle_room(
        &self,
        request: tonic::Request<JoinShuffleRoomRequest>,
    ) -> Result<tonic::Response<JoinShuffleRoomResponse>, tonic::Status> {
        let request = request.into_inner();

        let utxo_id = U256::from_big_endian(&request.utxo_id);

        let utxo = self
            .utxo_contract
            .get_utxo_by_id(utxo_id)
            .await
            .map_err(|err| {
                log::error!("failed to get utxo from contract: {err}");
                tonic::Status::internal("failed to get utxo from contract")
            })?
            .ok_or_else(|| {
                log::debug!("utxo with id {utxo_id} not found");
                tonic::Status::invalid_argument("no utxo with such id")
            })?;

        verify_join_signature(&utxo.id, request.timestamp, request.signature, utxo.owner).map_err(
            |err| {
                log::debug!("failed to verify join signature: {err}");
                tonic::Status::invalid_argument("invalid signature or timestamp")
            },
        )?;

        self.inner
            .add_participant(&utxo.token, &utxo.amount, &utxo.id)
            .await
            .map_err(|err| {
                log::error!("failed to add participant: {err}");
                tonic::Status::internal("failed to add participant")
            })?;

        let queue_length = self
            .storage
            .queue_length(&utxo.token, &utxo.amount)
            .await
            .map_err(|err| {
                log::error!("failed to get queue length: {err}");
                tonic::Status::internal("failed to get queue length")
            })?;

        if queue_length >= MIN_ROOM_SIZE {
            self.inner
                .create_rooms(&utxo.token, &utxo.amount)
                .await
                .map_err(|err| {
                    log::error!("failed to start shuffle: {err}");
                    tonic::Status::internal("failed to start shuffle")
                })?;
        }

        Ok(tonic::Response::new(JoinShuffleRoomResponse {
            room_access_token: self
                .tokens_generator
                .generate_token(utxo.token, utxo.amount, utxo.id)
                .map_err(|err| {
                    log::error!("failed to generate token: {err}");
                    tonic::Status::internal("failed to generate token")
                })?,
        }))
    }

    async fn is_ready_for_shuffle(
        &self,
        request: tonic::Request<IsReadyForShuffleRequest>,
    ) -> Result<tonic::Response<IsReadyForShuffleResponse>, tonic::Status> {
        let claims = self
            .tokens_generator
            .decode_token(&request)
            .map_err(|err| {
                log::debug!("failed to decode token: {err}");
                tonic::Status::unauthenticated("invalid token")
            })?;

        let is_room_ready = self
            .inner
            .get_participant(&claims.utxo_id)
            .await
            .map_err(|err| {
                log::error!("failed to get participant: {err}");
                tonic::Status::unauthenticated("no participant with such id")
            })?
            .room_id
            .is_some();

        let new_token = self
            .tokens_generator
            .generate_token(claims.token, claims.amount, claims.utxo_id)
            .map_err(|err| {
                log::error!("failed to generate token: {err}");
                tonic::Status::internal("failed to generate token")
            })?;

        // if participant is not in the room, it means that the shuffle is not started yet
        Ok(tonic::Response::new(IsReadyForShuffleResponse {
            ready: is_room_ready,
            room_access_token: new_token,
        }))
    }

    type ConnectShuffleRoomStream = ReceiverStream<Result<ShuffleEvent, tonic::Status>>;

    async fn connect_shuffle_room(
        &self,
        request: tonic::Request<ConnectShuffleRoomRequest>,
    ) -> Result<tonic::Response<Self::ConnectShuffleRoomStream>, tonic::Status> {
        // TODO: move to separate method
        let claims = self
            .tokens_generator
            .decode_token(&request)
            .map_err(|err| {
                log::debug!("failed to decode token: {err}");
                tonic::Status::unauthenticated("invalid token")
            })?;

        let (events_sender, events_receiver) = tokio::sync::mpsc::channel(10);

        let room = RoomConnection::new(events_receiver);

        let rooms_lock = self.rooms.lock().await;

        let room_id = Uuid::new_v4(); // FIXME: get room id from participant

        rooms_lock.insert(room_id, events_sender);

        let timeout_secs = Duration::from_secs(60 * 60 * 24); // TODO: get from room

        // tokio::spawn(async {
        //     if let Err(_) = timeout(timeout_secs, room.run()).await {
        //         log::error!("failed to receive event from room");
        //     }
        // });

        let (tx, rx) = tokio::sync::mpsc::channel(10);

        room.add_participant(claims.utxo_id, tx);

        Ok(tonic::Response::new(ReceiverStream::new(rx)))
    }

    async fn shuffle_round(
        &self,
        _request: tonic::Request<ShuffleRoundRequest>,
    ) -> Result<tonic::Response<ShuffleRoundResponse>, tonic::Status> {
        unimplemented!()
    }

    async fn sign_shuffle_tx(
        &self,
        _request: tonic::Request<SignShuffleTxRequest>,
    ) -> Result<tonic::Response<SignShuffleTxResponse>, tonic::Status> {
        unimplemented!()
    }
}
