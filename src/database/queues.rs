use async_trait::async_trait;
use coin_shuffle_core::service::storage::queues::{Error, Storage};
use ethers_core::{abi::Address, types::U256};
use sqlx::types::{BigDecimal, Json};

use super::{utils::u256_to_big_decimal, Database};

struct QueuesRow {
    #[allow(dead_code)]
    token: String,
    #[allow(dead_code)]
    amount: BigDecimal,
    participants: Json<Vec<U256>>,
}

#[async_trait]
impl Storage for Database {
    async fn push_to_queue(
        &self,
        token: &Address,
        amount: &U256,
        participant: &U256,
    ) -> Result<(), Error> {
        let row = self.get_queue(token, amount).await?;

        if let Some(row) = row {
            let mut participants = row.participants.0;
            participants.push(*participant);

            self.update_queue(token, amount, &participants).await?;
        } else {
            self.insert_queue(token, amount, &vec![*participant])
                .await?;
        }

        Ok(())
    }

    async fn pop_from_queue(
        &self,
        token: &Address,
        amount: &U256,
        number: usize,
    ) -> Result<Vec<U256>, Error> {
        let row = self
            .get_queue(token, amount)
            .await?
            .ok_or(Error::Internal("not found".into()))?;

        let amount = u256_to_big_decimal(amount).map_err(|e| Error::Internal(e.to_string()))?;

        let mut participants = row.participants.0;

        let poped_participants = participants.drain(..number).collect::<Vec<U256>>();

        sqlx::query!(
            r#"
                UPDATE queues
                SET participants = $1
                WHERE token = $2 AND amount = $3
            "#,
            Json(participants) as _,
            format!("{:?}", token),
            amount
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(poped_participants)
    }

    async fn queue_len(&self, token: &Address, amount: &U256) -> Result<usize, Error> {
        let row = self.get_queue(token, amount).await?;

        let Some(row) = row else {
            return Ok(0);
        };

        Ok(row.participants.0.len())
    }
}

impl Database {
    async fn get_queue(&self, token: &Address, amount: &U256) -> Result<Option<QueuesRow>, Error> {
        let amount = u256_to_big_decimal(amount).map_err(|err| Error::Internal(err.to_string()))?;

        let row = sqlx::query_as!(
            QueuesRow,
            r#"
                SELECT token, amount, participants as "participants: Json<Vec<U256>>"
                FROM queues
                WHERE token = $1 AND amount = $2
            "#,
            format!("{:?}", token),
            amount,
        )
        .fetch_optional(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(row)
    }

    async fn insert_queue(
        &self,
        token: &Address,
        amount: &U256,
        participants: &Vec<U256>,
    ) -> Result<(), Error> {
        let amount = u256_to_big_decimal(amount).map_err(|err| Error::Internal(err.to_string()))?;

        sqlx::query!(
            r#"
                INSERT INTO queues (token, amount, participants)
                VALUES ($1, $2, $3)
            "#,
            format!("{:?}", token),
            amount,
            Json(participants) as _,
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }

    async fn update_queue(
        &self,
        token: &Address,
        amount: &U256,
        participants: &Vec<U256>,
    ) -> Result<(), Error> {
        let amount = u256_to_big_decimal(amount).map_err(|err| Error::Internal(err.to_string()))?;

        sqlx::query!(
            r#"
                UPDATE queues
                SET participants = $1
                WHERE token = $2 AND amount = $3
            "#,
            Json(participants) as _,
            format!("{:?}", token),
            amount,
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }
}
