mod auth;
mod room;

use coin_shuffle_contracts_bindings::utxo::{self, Contract};
use coin_shuffle_core::service::Service;
use coin_shuffle_protos::v1::{
    shuffle_service_server::ShuffleService, ConnectShuffleRoomRequest, IsReadyForShuffleRequest,
    IsReadyForShuffleResponse, JoinShuffleRoomRequest, JoinShuffleRoomResponse, ShuffleEvent,
    ShuffleRoundRequest, ShuffleRoundResponse, SignShuffleTxRequest, SignShuffleTxResponse,
};
use ethers_core::abi::ethereum_types::Signature;
use ethers_core::types::U256;
use ethers_middleware::SignerMiddleware;
use ethers_providers::{Http, Provider};
use ethers_signers::LocalWallet;
use rsa::{BigUint, RsaPublicKey};
use std::collections::hash_map::Entry::Vacant;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Sender as StreamSender;
use tokio::sync::Mutex;
use tokio::time::{interval_at, Instant};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::waiter::Waiter;

use self::{
    auth::{verify_join_signature, TokensGenerator},
    room::{RoomConnectionManager, RoomEvents},
};

// TODO: Separate requests data parsing level

pub struct Protocol {
    service: Service,
    utxo_contract: utxo::Connector<SignerMiddleware<Provider<Http>, LocalWallet>>,
    tokens_generator: TokensGenerator,

    shuffle_round_deadline: Duration,

    waiter: Waiter,
    rooms: Arc<Mutex<HashMap<Uuid, StreamSender<RoomEvents>>>>,
}

impl Protocol {
    pub fn new(
        contract: utxo::Connector<SignerMiddleware<Provider<Http>, LocalWallet>>,
        token_key: String,
        shuffle_round_deadline: Duration,
        min_room_size: usize,
    ) -> Self {
        Self {
            shuffle_round_deadline,
            waiter: Waiter::new(min_room_size),
            service: Service::new(),
            utxo_contract: contract,
            tokens_generator: TokensGenerator::new(token_key),
            rooms: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[tonic::async_trait]
impl ShuffleService for Protocol {
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

        if let Some(participants) = self
            .waiter
            .add_participant(utxo.token, utxo.amount, utxo.id)
            .await
        {
            self.service
                .create_room(utxo.token, utxo.amount, participants)
                .await;
        };

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

        let participant = self.service.get_participant(&claims.utxo_id).await;

        let new_token = self
            .tokens_generator
            .generate_shuffle_token(claims.token, claims.amount, claims.utxo_id)
            .map_err(|err| {
                log::error!("failed to generate token: {err}");
                tonic::Status::internal("internal error")
            })?;

        // if participant is not in the room, it means that the shuffle is not started yet
        Ok(tonic::Response::new(IsReadyForShuffleResponse {
            ready: participant.is_some(), // participant is created when room is created
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
            .service
            .get_participant(&claims.utxo_id)
            .await
            .ok_or_else(|| {
                log::debug!("participant is absent, utxo_id: {}", claims.utxo_id);
                tonic::Status::not_found("participant is absent")
            })?;

        let room_id = participant.room_id;

        let room_stream = self.get_room_stream(room_id).await.ok_or_else(|| {
            log::error!("failed to find the room with id: {}", room_id);
            tonic::Status::internal("internal error")
        })?;

        let (event_sender, event_receiver) = channel(10);

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
            .send(RoomEvents::AddParticipant {
                utxo_id: participant.utxo_id,
                stream: event_sender,
                key: rsa_public_key,
            })
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

impl Protocol {
    async fn get_room_stream(&self, room_id: Uuid) -> Option<StreamSender<RoomEvents>> {
        let mut rooms = self.rooms.lock().await;

        if let Vacant(e) = rooms.entry(room_id) {
            let (internal_events_sender, internal_events_receiver) = channel(10);
            let mut room = RoomConnectionManager::new(
                internal_events_receiver,
                self.service.get_room(&room_id).await?, // TODO: check if this behaviour is valid
                self.service.clone(),
                self.tokens_generator.clone(),
                self.utxo_contract.clone(),
            );
            room.set_deadline(interval_at(
                Instant::now() + self.shuffle_round_deadline,
                self.shuffle_round_deadline,
            ));

            tokio::spawn(async move {
                room.run().await;
            });

            e.insert(internal_events_sender.clone());

            return Some(internal_events_sender);
        }

        rooms.get(&room_id).cloned()
    }
}
