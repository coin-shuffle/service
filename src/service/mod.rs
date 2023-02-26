mod auth;

use coin_shuffle_contracts_bindings::utxo::Contract;
use coin_shuffle_core::service::{
    storage::Storage, types::participant, waiter::simple::SimpleWaiter, Service as Core,
};
use coin_shuffle_protos::v1::{
    shuffle_service_server::ShuffleService, ConnectShuffleRoomRequest, IsReadyForShuffleRequest,
    IsReadyForShuffleResponse, JoinShuffleRoomRequest, JoinShuffleRoomResponse, ShuffleEvent,
    ShuffleRoundRequest, ShuffleRoundResponse, SignShuffleTxRequest, SignShuffleTxResponse,
};
use ethers_core::types::U256;
use tokio_stream::wrappers::ReceiverStream;

use self::auth::{verify_join_signature, TokensGenerator};

pub struct Service<S, C>
where
    S: Storage,
    C: Contract,
{
    inner: Core<S, SimpleWaiter<S>, C>,
    utxo_contract: C,
    storage: S,
    tokens_generator: TokensGenerator,
}

pub const MIN_ROOM_SIZE: usize = 3;

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
                tonic::Status::internal("failed to utxo from contract")
            })?
            .ok_or_else(|| {
                log::debug!("utxo with id {utxo_id} not found");
                tonic::Status::invalid_argument("no utxo with such id")
            })?;

        verify_join_signature(&utxo_id, request.timestamp, request.signature, utxo.owner).map_err(
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

        let token = self
            .tokens_generator
            .generate_token(utxo.token, utxo.amount, utxo.id)
            .map_err(|err| {
                log::error!("failed to generate token: {err}");
                tonic::Status::internal("failed to generate token")
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
            room_access_token: token,
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

        let participant = self
            .inner
            .get_participant(&claims.utxo_id)
            .await
            .map_err(|err| {
                log::error!("failed to get participant: {err}");
                tonic::Status::unauthenticated("no participant with such id")
            })?;

        let new_token = self
            .tokens_generator
            .generate_token(claims.token, claims.amount, claims.utxo_id)
            .map_err(|err| {
                log::error!("failed to generate token: {err}");
                tonic::Status::internal("failed to generate token")
            })?;

        // if participant is not in the room, it means that the shuffle is not started yet
        Ok(tonic::Response::new(IsReadyForShuffleResponse {
            ready: participant.room_id.is_some(),
            room_access_token: new_token,
        }))
    }

    type ConnectShuffleRoomStream = ReceiverStream<Result<ShuffleEvent, tonic::Status>>;

    async fn connect_shuffle_room(
        &self,
        _request: tonic::Request<ConnectShuffleRoomRequest>,
    ) -> Result<tonic::Response<Self::ConnectShuffleRoomStream>, tonic::Status> {
        unimplemented!()
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
