use std::{
    ops::Deref,
    sync::Arc,
    time::{Duration, SystemTime},
};

use ethers_core::types::{Address, RecoveryMessage, Signature, U256};
use eyre::{eyre, Context, ContextCompat};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use open_fastrlp::Decodable;
use uuid::Uuid;

const U256_BYTES: usize = 32;
const TIMESTAMP_BYTES: usize = 8;
const MESSAGE_LEN: usize = U256_BYTES + TIMESTAMP_BYTES;

pub fn verify_join_signature(
    utxo_id: &U256,
    timestamp: u64,
    signature: Vec<u8>,
    owner: impl Into<Address>,
) -> Result<(), JoinSignatureError> {
    let mut message = vec![0u8; MESSAGE_LEN];

    utxo_id.to_big_endian(&mut message[0..U256_BYTES]);
    message[U256_BYTES..MESSAGE_LEN].copy_from_slice(&timestamp.to_be_bytes());

    dbg!(&signature);

    let signature = Signature::decode(&mut signature.deref())
        .map_err(|err| JoinSignatureError::InvalidSignature(eyre!("failed to decode: {err}")))?;

    let now = SystemTime::now();
    let signature_creation_time = SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp);

    if now.duration_since(signature_creation_time).is_err() {
        return Err(JoinSignatureError::InvalidTimestamp(timestamp));
    }

    let msg = RecoveryMessage::Data(message);

    signature
        .verify(msg, owner)
        .map_err(|err| JoinSignatureError::InvalidSignature(eyre!("invalid signature: {err}")))
}

#[derive(thiserror::Error, Debug)]
pub enum JoinSignatureError {
    #[error("Invalid signature: {0}")]
    InvalidSignature(eyre::Error),
    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(u64),
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ShuffleAccessClaim {
    pub token: Address,
    pub amount: U256,
    pub utxo_id: U256,
    pub exp: usize,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RoomAccessClaim {
    pub utxo_id: U256,
    pub room_id: Uuid,
    pub exp: usize,
}

#[derive(Clone)]
pub struct TokensGenerator {
    secret_key: Arc<String>,
}

impl TokensGenerator {
    pub fn new(secret_key: String) -> Self {
        Self {
            secret_key: Arc::new(secret_key),
        }
    }

    pub fn generate_shuffle_token(
        &self,
        token: Address,
        amount: U256,
        utxo_id: U256,
    ) -> Result<String, eyre::Error> {
        let exp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs() as usize
            + 60 * 60 * 24;

        let claim = ShuffleAccessClaim {
            token,
            amount,
            utxo_id,
            exp,
        };

        jsonwebtoken::encode(
            &Header::default(),
            &claim,
            &EncodingKey::from_secret(self.secret_key.as_bytes()),
        )
        .context("failed to generate token")
    }

    pub fn generate_room_token(&self, room_id: Uuid, utxo_id: U256) -> Result<String, eyre::Error> {
        let exp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs() as usize
            + 60 * 60 * 24;

        let claim = RoomAccessClaim {
            utxo_id,
            room_id,
            exp,
        };

        jsonwebtoken::encode(
            &Header::default(),
            &claim,
            &EncodingKey::from_secret(self.secret_key.as_bytes()),
        )
        .context("failed to generate token")
    }

    pub fn decode_shuffle_token<T>(
        &self,
        req: &tonic::Request<T>,
    ) -> eyre::Result<ShuffleAccessClaim> {
        let token = req
            .metadata()
            .get("authorization")
            .context("missing authorization header")?
            .to_str()?
            .strip_prefix("Bearer ")
            .context("invalid authorization header")?;

        let token = jsonwebtoken::decode::<ShuffleAccessClaim>(
            token,
            &DecodingKey::from_secret(self.secret_key.as_bytes()),
            &Validation::default(), // TODO: add validation
        )
        .context("failed to decode token")?
        .claims;

        Ok(token)
    }

    pub fn decode_room_token<T>(&self, req: &tonic::Request<T>) -> eyre::Result<RoomAccessClaim> {
        let token = req
            .metadata()
            .get("authorization")
            .context("missing authorization header")?
            .to_str()?
            .strip_prefix("Bearer ")
            .context("invalid authorization header")?;

        let token = jsonwebtoken::decode::<RoomAccessClaim>(
            token,
            &DecodingKey::from_secret(self.secret_key.as_bytes()),
            &Validation::default(), // TODO: add validation
        )
        .context("failed to decode token")?
        .claims;

        Ok(token)
    }
}
