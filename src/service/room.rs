use coin_shuffle_contracts_bindings::utxo::Contract;
use coin_shuffle_core::service::storage::{participants, Storage};
use coin_shuffle_core::service::types::{EncodedOutput, ShuffleRound};
use coin_shuffle_core::service::waiter::simple::SimpleWaiter;
use coin_shuffle_core::service::Service as Core;
use coin_shuffle_core::types::Output;
use coin_shuffle_protos::v1::shuffle_event::Body;
use coin_shuffle_protos::v1::{
    EncodedOutputs, RsaPublicKey as ProtosRsaPublicKey, ShuffleTxHash, TxSigningOutputs,
};
use std::collections::HashMap;

use crate::service::auth::TokensGenerator;
use crate::service::MIN_ROOM_SIZE;
use coin_shuffle_protos::v1::{ShuffleEvent, ShuffleInfo};
use ethers_core::abi::ethereum_types::Signature;
use ethers_core::types::U256;
use eyre::{Context, Result};
use rsa::{PublicKeyParts, RsaPublicKey};
use tokio::sync::mpsc::{Receiver as StreamReceiver, Sender as StreamSender};
use tokio::time::{Duration, Interval};
use uuid::Uuid;

pub const ROUND_DEADLINE: Duration = Duration::from_secs(2 * 60); // TODO: Move to config

#[derive(Debug, Clone)]
pub enum RoomEvents {
    ShuffleRound((U256, Vec<EncodedOutput>)),
    SignedOutput((U256, Signature)),
    AddParticipant(
        (
            U256,
            StreamSender<Result<ShuffleEvent, tonic::Status>>,
            RsaPublicKey,
        ),
    ),
}

pub struct RoomConnection<S, C>
where
    S: Storage + Clone,
    C: Contract + Clone,
{
    room_id: Uuid,
    deadline: Interval,
    events: StreamReceiver<RoomEvents>,
    participant_streams: HashMap<U256, StreamSender<Result<ShuffleEvent, tonic::Status>>>,
    core: Core<S, SimpleWaiter<S>, C>,
    token_generator: TokensGenerator,
}

impl<S, C> RoomConnection<S, C>
where
    S: Storage + Clone + 'static,
    C: Contract + Send + Sync + Clone + 'static,
{
    pub fn new(
        events: StreamReceiver<RoomEvents>,
        deadline: Interval,
        room_id: Uuid,
        core: Core<S, SimpleWaiter<S>, C>,
        token_generator: TokensGenerator,
    ) -> Self {
        Self {
            deadline,
            events,
            participant_streams: HashMap::new(),
            room_id,
            core,
            token_generator,
        }
    }

    pub async fn run(&mut self) {
        log::info!("new room is opened: {}", self.room_id);
        loop {
            tokio::select! {
                _ = self.deadline.tick() => {
                    // TODO: Add the huilo list returning
                    log::debug!("[ROOM][{}] deadline is over", self.room_id);
                    return;
                }
                Some(event) = self.events.recv() => {
                    match self.handle_event(event.clone()).await {
                        Err(err) => {
                            log::error!("[ROOM][{}] {err}", self.room_id);
                            return
                        }
                        Ok(..) => {
                            log::debug!("[ROOM][{}] event {:?} handled", self.room_id, event)
                        }
                    }
                }
            }
        }
    }

    pub async fn handle_event(&mut self, event: RoomEvents) -> Result<()> {
        match event {
            RoomEvents::AddParticipant((utxo_id, event_stream, public_key)) => {
                self.event_add_participant(utxo_id, event_stream, public_key)
                    .await?;
            }
            RoomEvents::ShuffleRound((utxo_id, decoded_outputs)) => {
                self.event_shuffle_round(utxo_id, decoded_outputs).await?
            }
            RoomEvents::SignedOutput((utxo_id, signature)) => {
                self.event_signed_output(utxo_id, signature).await?
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
            "[EVENT][{}] add participant handling: {}...",
            self.room_id,
            utxo_id
        );
        self.participant_streams.insert(utxo_id, stream);

        self.core
            .add_participant_key(&utxo_id, public_key)
            .await
            .context(format!(
                "failed to add participant public key, utxo id: {utxo_id}"
            ))?;

        if self.participant_streams.len() == MIN_ROOM_SIZE {
            self.distribute_public_keys().await?;
            self.send_outputs(&utxo_id, Vec::new()).await?;
        }
        log::info!(
            "[EVENT][{}] add participant handled: {}",
            self.room_id,
            utxo_id
        );

        Ok(())
    }

    pub async fn event_shuffle_round(
        &self,
        utxo_id: U256,
        decoded_outputs: Vec<EncodedOutput>,
    ) -> Result<()> {
        log::info!("[EVENT][{}] shuffle round: {}...", self.room_id, utxo_id);
        let current_round = self
            .core
            .pass_decoded_outputs(&utxo_id, decoded_outputs)
            .await?;
        log::info!(
            "[EVENT] shuffle round current round: {}, part: {}",
            current_round,
            utxo_id
        );

        if current_round == self.participant_streams.len() {
            self.distribute_outputs().await?;
        }
        log::info!("[EVENT] shuffle round: {}", utxo_id);

        Ok(())
    }

    pub async fn event_signed_output(&self, utxo_id: U256, signature: Signature) -> Result<()> {
        log::info!("[EVENT] signed output: {}...", utxo_id);
        self.core
            .pass_outputs_signature(&utxo_id, signature)
            .await
            .context("failed to save output signature")?;

        let is_signature_passed = self
            .core
            .is_signature_passed(&self.room_id)
            .await
            .context("failed to check is all signature have passed")?;

        if is_signature_passed {
            let tx_hash = self
                .core
                .send_transaction(&self.room_id)
                .await
                .context("failed to send transaction")?;

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
        }
        log::info!("[EVENT] signed output: {}", utxo_id);

        Ok(())
    }

    pub async fn distribute_public_keys(&self) -> Result<()> {
        let room = self
            .core
            .get_room(&self.room_id)
            .await
            .context(format!("failed to get room by id: {}", self.room_id))?;

        for utxo_id in room.participants.iter() {
            let public_keys_list = self
                .core
                .participant_keys(utxo_id)
                .await
                .context("failed to get participant public keys")?;

            let mut public_keys_list_raw: Vec<ProtosRsaPublicKey> = Vec::new();
            for public_key in public_keys_list {
                public_keys_list_raw.push(ProtosRsaPublicKey {
                    modulus: public_key.n().to_bytes_be(),
                    exponent: public_key.e().to_bytes_be(),
                })
            }

            public_keys_list_raw.reverse();

            let shuffle_access_token = self
                .token_generator
                .generate_room_token(self.room_id, *utxo_id)
                .context(format!(
                    "failed to generate room access token, room id: {}, utxo id: {utxo_id}",
                    self.room_id
                ))?;

            self.participant_streams[utxo_id]
                .send(Ok(ShuffleEvent {
                    body: Some(Body::ShuffleInfo(ShuffleInfo {
                        public_keys_list: public_keys_list_raw,
                        shuffle_access_token,
                    })),
                }))
                .await?
        }

        Ok(())
    }

    /// Send ouputs to participant
    async fn send_outputs(&self, participants: &U256, outputs: Vec<EncodedOutput>) -> Result<()> {
        self.participant_streams[participants]
            .send(Ok(ShuffleEvent {
                body: Some(Body::EncodedOutputs(EncodedOutputs { outputs })),
            }))
            .await
            .context("failed to send outputs to participant")
    }

    pub async fn distribute_outputs(&self) -> Result<()> {
        let room = self
            .core
            .get_room(&self.room_id)
            .await
            .context(format!("failed to get room by id: {}", self.room_id))?;

        for utxo_id in room.participants.iter() {
            let outputs = self
                .core
                .decoded_outputs(&self.room_id)
                .await
                .context("failed to get decoded outputs")?;

            let mut outputs_raw: Vec<Vec<u8>> = Vec::new();
            for output in outputs {
                outputs_raw.push(output.as_bytes().to_vec())
            }

            self.participant_streams[utxo_id]
                .send(Ok(ShuffleEvent {
                    body: Some(Body::TxSigningOutputs(TxSigningOutputs {
                        outputs: outputs_raw,
                    })),
                }))
                .await?
        }

        Ok(())
    }
}
