mod auth;
mod room;

use crate::service::room::ROUND_DEADLINE;
use coin_shuffle_contracts_bindings::utxo::Contract;
use coin_shuffle_core::service::{storage::Storage, waiter::simple::SimpleWaiter, Service as Core};
use coin_shuffle_protos::v1::{
    shuffle_service_server::ShuffleService, ConnectShuffleRoomRequest, IsReadyForShuffleRequest,
    IsReadyForShuffleResponse, JoinShuffleRoomRequest, JoinShuffleRoomResponse, ShuffleEvent,
    ShuffleRoundRequest, ShuffleRoundResponse, SignShuffleTxRequest, SignShuffleTxResponse,
};
use ethers_core::abi::ethereum_types::Signature;
use ethers_core::types::U256;
use rsa::{BigUint, RsaPublicKey};
use std::collections::hash_map::Entry::Vacant;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Sender as StreamSender;
use tokio::sync::Mutex;
use tokio::time::{interval_at, Instant};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use self::{
    auth::{verify_join_signature, TokensGenerator},
    room::{RoomConnection, RoomEvents},
};

// TODO: Separate requests data parsing level

pub struct Service<S, C>
where
    S: Storage + Clone,
    C: Contract + Clone,
{
    inner: Core<S, SimpleWaiter<S>, C>,
    utxo_contract: C,
    storage: S,
    tokens_generator: TokensGenerator,

    rooms: Arc<Mutex<HashMap<Uuid, StreamSender<RoomEvents>>>>,
}

impl<S, C> Service<S, C>
where
    S: Storage,
    C: Contract + Clone,
{
    pub fn new(contract: C, storage: S, token_key: String) -> Self {
        Self {
            inner: Core::new(
                storage.clone(),
                SimpleWaiter::new(MIN_ROOM_SIZE, storage.clone()),
                contract.clone(),
            ),
            utxo_contract: contract,
            storage,
            tokens_generator: TokensGenerator::new(token_key),
            rooms: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub const MIN_ROOM_SIZE: usize = 4; // TODO: Move to config

#[tonic::async_trait]
impl<S, C> ShuffleService for Service<S, C>
where
    S: Storage + Clone + 'static,
    C: Contract + Send + Sync + Clone + 'static,
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
                tonic::Status::internal("internal error")
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
                tonic::Status::internal("internal error")
            })?;

        let queue_length = self
            .storage
            .queue_len(&utxo.token, &utxo.amount)
            .await
            .map_err(|err| {
                log::error!("failed to get queue length: {err}");
                tonic::Status::internal("internal error")
            })?;

        if queue_length >= MIN_ROOM_SIZE {
            self.inner
                .create_rooms(&utxo.token, &utxo.amount)
                .await
                .map_err(|err| {
                    log::error!("failed to start shuffle: {err}");
                    tonic::Status::internal("internal error")
                })?;
        }

        Ok(tonic::Response::new(JoinShuffleRoomResponse {
            room_access_token: self
                .tokens_generator
                .generate_shuffle_token(utxo.token, utxo.amount, utxo.id)
                .map_err(|err| {
                    log::error!("failed to generate token: {err}");
                    tonic::Status::internal("internal error")
                })?,
        }))
    }

    async fn is_ready_for_shuffle(
        &self,
        request: tonic::Request<IsReadyForShuffleRequest>,
    ) -> Result<tonic::Response<IsReadyForShuffleResponse>, tonic::Status> {
        let claims = self
            .tokens_generator
            .decode_shuffle_token(&request)
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
            .generate_shuffle_token(claims.token, claims.amount, claims.utxo_id)
            .map_err(|err| {
                log::error!("failed to generate token: {err}");
                tonic::Status::internal("internal error")
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
        let claims = self
            .tokens_generator
            .decode_shuffle_token(&request)
            .map_err(|err| {
                log::debug!("failed to decode token: {err}");
                tonic::Status::unauthenticated("invalid token")
            })?;

        let participant = self
            .inner
            .get_participant(&claims.utxo_id)
            .await
            .map_err(|err| {
                log::error!("failed to get participant: {err}");
                tonic::Status::internal("internal error")
            })?;

        let room_id = participant.room_id.ok_or_else(|| {
            log::debug!("room is absent");
            tonic::Status::not_found("room is absent")
        })?;

        let room_stream = self.get_room_stream(room_id).await.ok_or_else(|| {
            log::error!("failed to find the room with id: {}", room_id);
            tonic::Status::internal("internal error")
        })?;

        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(10);

        let rsa_public_key_raw = request.into_inner().public_key.ok_or_else(|| {
            log::debug!("public key is missing, utxo_id: {}", claims.utxo_id);
            tonic::Status::invalid_argument("public key is missing")
        })?;

        let rsa_public_key = RsaPublicKey::new(
            BigUint::from_bytes_be(rsa_public_key_raw.modulus.as_slice()),
            BigUint::from_bytes_be(rsa_public_key_raw.exponent.as_slice()),
        )
        .map_err(|err| {
            log::error!("failed to parse rsa public key: {err}");
            tonic::Status::invalid_argument("invalid rsa public key")
        })?;

        room_stream
            .send(RoomEvents::AddParticipant((
                participant.utxo_id,
                event_sender,
                rsa_public_key,
            )))
            .await
            .map_err(|err| {
                log::error!(
                    "failed to add user to room, utxo_id: {}, room_id: {room_id}: {err}",
                    participant.utxo_id,
                );
                tonic::Status::internal("internal error")
            })?;

        Ok(tonic::Response::new(ReceiverStream::new(event_receiver)))
    }

    async fn shuffle_round(
        &self,
        request: tonic::Request<ShuffleRoundRequest>,
    ) -> Result<tonic::Response<ShuffleRoundResponse>, tonic::Status> {
        let claims = self
            .tokens_generator
            .decode_room_token(&request)
            .map_err(|err| {
                log::debug!("failed to decode token: {err}");
                tonic::Status::unauthenticated("invalid token")
            })?;

        let room_stream = self.get_room_stream(claims.room_id).await.ok_or_else(|| {
            log::error!("failed to find the room with id: {}", claims.room_id);
            tonic::Status::internal("internal error")
        })?;

        room_stream
            .send(RoomEvents::ShuffleRound((
                claims.utxo_id,
                request.into_inner().encoded_outputs,
            )))
            .await
            .map_err(|err| {
                log::error!(
                    "failed to internal send shuffle round event, utxo_id: {}, room_id: {}: {err}",
                    claims.utxo_id,
                    claims.room_id,
                );
                tonic::Status::internal("internal error")
            })?;

        Ok(tonic::Response::new(ShuffleRoundResponse {}))
    }

    async fn sign_shuffle_tx(
        &self,
        request: tonic::Request<SignShuffleTxRequest>,
    ) -> Result<tonic::Response<SignShuffleTxResponse>, tonic::Status> {
        let claims = self
            .tokens_generator
            .decode_room_token(&request)
            .map_err(|err| {
                log::debug!("failed to decode token: {err}");
                tonic::Status::unauthenticated("invalid token")
            })?;

        let room_stream = self.get_room_stream(claims.room_id).await.ok_or_else(|| {
            log::error!("failed to find the room with id: {}", claims.room_id);
            tonic::Status::internal("internal error")
        })?;

        room_stream
            .send(RoomEvents::SignedOutput((
                claims.utxo_id,
                Signature::from_slice(request.into_inner().signature.as_slice()),
            )))
            .await
            .map_err(|err| {
                log::error!(
                    "failed to internal send shuffle round event, utxo_id: {}, room_id: {}: {err}",
                    claims.utxo_id,
                    claims.room_id,
                );
                tonic::Status::internal("internal error")
            })?;

        Ok(tonic::Response::new(SignShuffleTxResponse {}))
    }
}

impl<S, C> Service<S, C>
where
    S: Storage + Clone + 'static,
    C: Contract + Send + Sync + Clone + 'static,
{
    async fn get_room_stream(&self, room_id: Uuid) -> Option<StreamSender<RoomEvents>> {
        let mut rooms_lock = self.rooms.lock().await;
        if let Vacant(e) = rooms_lock.entry(room_id) {
            let (internal_events_sender, internal_events_receiver) = channel(10);
            let mut room = RoomConnection::new(
                internal_events_receiver,
                interval_at(Instant::now() + ROUND_DEADLINE, ROUND_DEADLINE),
                room_id,
                self.inner.clone(),
                self.tokens_generator.clone(),
            );

            tokio::spawn(async move {
                room.run().await;
            });

            e.insert(internal_events_sender.clone());

            return Some(internal_events_sender);
        }

        rooms_lock.get(&room_id).cloned()
    }
}
