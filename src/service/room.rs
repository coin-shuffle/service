use crate::service::auth::TokensGenerator;
use coin_shuffle_contracts_bindings::utxo::types::Output;
use coin_shuffle_contracts_bindings::utxo::{self, Contract};
use coin_shuffle_core::service::types::Room;
use ethers_middleware::SignerMiddleware;
use ethers_providers::{Http, Provider};
use ethers_signers::LocalWallet;

use coin_shuffle_core::service::{types::EncodedOutput, Service};
use coin_shuffle_protos::v1::{
    shuffle_event::Body, EncodedOutputs, RsaPublicKey as ProtosRsaPublicKey, ShuffleTxHash,
    TxSigningOutputs,
};
use coin_shuffle_protos::v1::{ShuffleError, ShuffleEvent, ShuffleInfo};
use ethers_core::{abi::ethereum_types::Signature, types::U256};
use eyre::{Context, Result};
use rsa::{PublicKeyParts, RsaPublicKey};
use std::collections::HashMap;
use tokio::{
    sync::mpsc::{Receiver as StreamReceiver, Sender as StreamSender},
    time::{interval_at, Duration, Instant, Interval},
};

pub const DEFAULT_ROUND_DEADLINE: Duration = Duration::from_secs(2 * 60);

#[derive(Debug, Clone)]
pub enum RoomEvents {
    ShuffleRound((U256, Vec<EncodedOutput>)),
    SignedOutput((U256, Signature)),
    AddParticipant {
        utxo_id: U256,
        stream: StreamSender<Result<ShuffleEvent, tonic::Status>>,
        key: RsaPublicKey,
    },
}

pub struct RoomConnectionManager {
    room: Room,

    deadline: Interval,
    events: StreamReceiver<RoomEvents>,
    participant_streams: HashMap<U256, StreamSender<Result<ShuffleEvent, tonic::Status>>>,
    service: Service,
    utxo_contract: utxo::Connector<SignerMiddleware<Provider<Http>, LocalWallet>>,
    token_generator: TokensGenerator,
}

impl RoomConnectionManager {
    pub fn new(
        events: StreamReceiver<RoomEvents>,
        room: Room,
        service: Service,
        token_generator: TokensGenerator,
        contract: utxo::Connector<SignerMiddleware<Provider<Http>, LocalWallet>>,
    ) -> Self {
        Self {
            service,
            events,
            room,
            token_generator,
            utxo_contract: contract,
            participant_streams: HashMap::new(),
            deadline: interval_at(
                Instant::now() + DEFAULT_ROUND_DEADLINE,
                DEFAULT_ROUND_DEADLINE,
            ),
        }
    }

    pub fn set_deadline(&mut self, deadline: Interval) -> &mut RoomConnectionManager {
        self.deadline = deadline;
        self
    }

    pub async fn run(&mut self) {
        log::info!("New room is opened: {}", self.room.id);
        loop {
            tokio::select! {
                _ = self.deadline.tick() => {
                    // TODO: Add the huilo list returning
                    log::debug!(target: "room", "room_id={} deadline is over", self.room.id);
                    return;
                }
                Some(event) = self.events.recv() => {
                    log::debug!(target: "room", "room_id={} new event {:?}", self.room.id, event);

                    match self.handle_event(event.clone()).await {
                        Err(err) => {
                            log::error!(target: "room", "room_id={} {err:?}", self.room.id);
                            for (_, stream) in self.participant_streams.iter_mut() {
                                let _ = stream.send(Ok(ShuffleEvent {
                                    body: Some(Body::Error(ShuffleError {
                                        error: format!("{:?}", err),
                                    })),
                                })).await;
                            }
                            self.service.clear_room(&self.room.id).await;
                            return
                        }
                        Ok(()) => {
                            log::debug!(target: "room", "room_id={} event handled", self.room.id);
                        }
                    }
                }
            }
        }
    }

    pub async fn handle_event(&mut self, event: RoomEvents) -> Result<()> {
        match event {
            RoomEvents::AddParticipant {
                utxo_id,
                stream,
                key,
            } => {
                self.event_add_participant(utxo_id, stream, key).await?;
            }
            RoomEvents::ShuffleRound((utxo_id, decoded_outputs)) => {
                self.event_shuffle_round(utxo_id, decoded_outputs).await?
            }
            RoomEvents::SignedOutput((utxo_id, signature)) => {
                self.event_signed_output(utxo_id, signature)
                    .await
                    .context("Failed to handle signed output")?;
            }
        }

        Ok(())
    }

    pub async fn event_add_participant(
        &mut self,
        utxo_id: U256,
        stream: StreamSender<Result<ShuffleEvent, tonic::Status>>,
        public_key: RsaPublicKey,
    ) -> Result<()> {
        log::info!(
            target: "event",
            "room_id={} add participant handling: {}...",
            self.room.id,
            utxo_id
        );
        self.participant_streams.insert(utxo_id, stream);

        let Some(distributed_keys) = self.service
            .connect_participant(&utxo_id, public_key)
            .await
            .context(format!(
                "failed to add participant public key, utxo id: {utxo_id}"
            ))? else {
                log::info!(
                    target: "event",
                    "room_id={} participant connected utxo_id={}",
                    self.room.id,
                    utxo_id
                );
                return Ok(()) // That means that still not all participants have connected;
            };

        self.distribute_public_keys(distributed_keys)
            .await
            .context("failed to distribute public keys")?;

        self.send_encoded_outputs(&self.room.participants[0], Vec::new())
            .await?;

        log::info!(
            target: "event",
            "room_id={} keys distributed after utxo_id={} connected",
            self.room.id,
            utxo_id
        );

        Ok(())
    }

    pub async fn event_shuffle_round(
        &self,
        utxo_id: U256,
        decoded_outputs: Vec<EncodedOutput>,
    ) -> Result<()> {
        log::info!(target: "event", "room_id={} shuffle round: utxo_id={} start", self.room.id, utxo_id);
        use coin_shuffle_core::service::PassDecodedOutputsResult::*;

        match self
            .service
            .pass_decoded_outputs(&utxo_id, decoded_outputs.clone())
            .await?
        {
            Finished(outputs) => self
                .distribute_outputs(outputs)
                .await
                .context("failed to distribute outputs")?,
            Round(current_round) => self
                .send_encoded_outputs(&self.room.participants[current_round], decoded_outputs)
                .await
                .context("failed to send outputs to the next participant")?,
        };

        log::info!(target: "event", "shuffle round: utxo_id={} end", utxo_id);

        Ok(())
    }

    pub async fn event_signed_output(&self, utxo_id: U256, signature: Signature) -> Result<()> {
        log::info!(target: "event", "room_id={} signed output: utxo_id={}", self.room.id, utxo_id);

        let Some((outputs, inputs)) = self.service
            .pass_signature(&self.room.id, &utxo_id, signature)
            .await
            .context("Failed to save output signature")? else {
                return Ok(()) // That means that still not all participants have signed outputs;
            };

        let tx_hash = self
            .utxo_contract
            .transfer(inputs, outputs)
            .await
            .context("Failed to send transaction")?;

        for (_, stream) in self.participant_streams.iter() {
            stream
                .send(Ok(ShuffleEvent {
                    body: Some(Body::ShuffleTxHash(ShuffleTxHash {
                        tx_hash: tx_hash.as_bytes().to_vec(),
                    })),
                }))
                .await
                .context("failed to send tx_hash to participant")?;
        }

        self.service.clear_room(&self.room.id).await;

        Ok(())
    }

    ///! Send event with RSA public keys that are required to decode outputs
    ///! to each participant.
    pub async fn distribute_public_keys(
        &self,
        keys: HashMap<U256, Vec<RsaPublicKey>>,
    ) -> Result<()> {
        for (utxo_id, participant_keys) in keys {
            let mut proto_keys: Vec<ProtosRsaPublicKey> = Vec::new();

            for public_key in participant_keys.iter() {
                proto_keys.push(ProtosRsaPublicKey {
                    modulus: public_key.n().to_bytes_be(),
                    exponent: public_key.e().to_bytes_be(),
                })
            }

            let shuffle_access_token = self
                .token_generator
                .generate_room_token(self.room.id, utxo_id)
                .context(format!(
                    "failed to generate room access token, room id: {}, utxo id: {utxo_id}",
                    self.room.id
                ))?;

            self.participant_streams[&utxo_id]
                .send(Ok(ShuffleEvent {
                    body: Some(Body::ShuffleInfo(ShuffleInfo {
                        public_keys_list: proto_keys,
                        shuffle_access_token,
                    })),
                }))
                .await?
        }

        Ok(())
    }

    async fn send_encoded_outputs(
        &self,
        participants: &U256,
        outputs: Vec<EncodedOutput>,
    ) -> Result<()> {
        self.participant_streams[participants]
            .send(Ok(ShuffleEvent {
                body: Some(Body::EncodedOutputs(EncodedOutputs { outputs })),
            }))
            .await
            .context("failed to send outputs to participant")
    }

    pub async fn distribute_outputs(&self, outputs: Vec<Output>) -> Result<()> {
        for utxo_id in self.room.participants.iter() {
            self.participant_streams[utxo_id]
                .send(Ok(ShuffleEvent {
                    body: Some(Body::TxSigningOutputs(TxSigningOutputs {
                        outputs: outputs
                            .iter()
                            .map(|o| o.owner.as_bytes().to_vec())
                            .collect(),
                    })),
                }))
                .await?
        }

        Ok(())
    }
}
