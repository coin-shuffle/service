use std::str::FromStr;

use async_trait::async_trait;
use coin_shuffle_core::rsa::RsaPublicKey;
use coin_shuffle_core::service::storage::participants::{Error, Storage};
use coin_shuffle_core::service::types::ShuffleRound;
use coin_shuffle_core::service::{storage::participants, types::Participant};
use ethers_core::types::U256;
use sqlx::types::{BigDecimal, Json};
use uuid::Uuid;

use super::utils::u256_to_big_decimal;
use super::{utils, Database};

struct ParticipantRow {
    #[allow(dead_code)]
    utxo_id: BigDecimal,

    value: Json<Participant>,
}

#[async_trait]
impl Storage for Database {
    async fn insert_participant(&self, participant: Participant) -> Result<(), Error> {
        let utxo_id = utils::u256_to_big_decimal(&participant.utxo_id)
            .map_err(|err| Error::Internal(err.to_string()))?;

        sqlx::query!(
            r#"
                INSERT INTO participants (utxo_id, value)
                VALUES ($1, $2)
            "#,
            utxo_id,
            Json(participant) as _,
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }

    async fn update_participant_room(
        &self,
        participant: &U256,
        room_id: &Uuid,
    ) -> Result<(), Error> {
        let participant = self
            .get_participant(participant)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
            .ok_or(Error::Internal("not found".into()))?;

        let participant = Participant {
            room_id: Some(*room_id),
            ..participant
        };

        let utxo_id = utils::u256_to_big_decimal(&participant.utxo_id)
            .map_err(|err| Error::Internal(err.to_string()))?;

        sqlx::query!(
            r#"
                UPDATE participants
                SET value = $1
                WHERE utxo_id = $2
            "#,
            Json(participant) as _,
            utxo_id,
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }

    async fn update_participant_round(
        &self,
        participant: &U256,
        round: ShuffleRound,
    ) -> Result<(), Error> {
        let participant = self
            .get_participant(participant)
            .await
            .map_err(|e| participants::Error::Internal(e.to_string()))?
            .ok_or(participants::Error::Internal("not found".into()))?;

        let participant = Participant {
            status: round,
            ..participant
        };

        let utxo_id = utils::u256_to_big_decimal(&participant.utxo_id)
            .map_err(|err| Error::Internal(err.to_string()))?;

        sqlx::query!(
            r#"
                UPDATE participants
                SET value = $1
                WHERE utxo_id = $2
            "#,
            Json(participant) as _,
            utxo_id,
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }

    async fn update_participant_key(
        &self,
        participant: &U256,
        key: RsaPublicKey,
    ) -> Result<(), Error> {
        let participant = self
            .get_participant(participant)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
            .ok_or(Error::Internal("not found".into()))?;

        let participant = Participant {
            rsa_pubkey: Some(key),
            ..participant
        };

        // convert `U256` to `BigDecimal`
        let utxo_id_as_str = participant.utxo_id.to_string();

        let utxo_id = BigDecimal::from_str(utxo_id_as_str.as_str())
            .map_err(|e| Error::Internal(e.to_string()))?;

        sqlx::query!(
            r#"
                UPDATE participants
                SET value = $1
                WHERE utxo_id = $2
            "#,
            Json(participant) as _,
            utxo_id,
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }

    async fn get_participant(&self, participant: &U256) -> Result<Option<Participant>, Error> {
        let utxo_id =
            u256_to_big_decimal(&participant).map_err(|err| Error::Internal(err.to_string()))?;

        let participant_row = sqlx::query_as!(
            ParticipantRow,
            r#"
                SELECT utxo_id, value as "value: Json<Participant>"
                FROM participants
                WHERE utxo_id = $1
            "#,
            utxo_id
        )
        .fetch_optional(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        let Some(participant_row) = participant_row else {
            return Ok(None);
        };

        Ok(Some(participant_row.value.0))
    }
}
