use async_trait::async_trait;
use coin_shuffle_core::service::{
    storage::rooms::{Error, Storage},
    types::Room,
};
use sqlx::types::Json;
use uuid::Uuid;

use super::Database;

struct RoomRow {
    #[allow(dead_code)]
    id: Uuid,

    value: Json<Room>,
}

#[async_trait]
impl Storage for Database {
    async fn get_room(&self, id: &Uuid) -> Result<Option<Room>, Error> {
        let room_row = sqlx::query_as!(
            RoomRow,
            r#"
                SELECT id, value as "value: Json<Room>"
                FROM rooms
                WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        let Some(room_row) = room_row else {
            return Ok(None);
        };

        Ok(Some(room_row.value.0))
    }

    async fn update_room_round(&self, id: &Uuid, round: usize) -> Result<(), Error> {
        let room = self
            .get_room(id)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
            .ok_or(Error::Internal("not found".into()))?;

        let room = Room {
            current_round: round,
            ..room
        };

        sqlx::query!(
            r#"
                UPDATE rooms
                SET value = $1
                WHERE id = $2
            "#,
            Json(room) as _,
            id
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }

    async fn insert_room(&self, room: &Room) -> Result<(), Error> {
        sqlx::query!(
            r#"
                INSERT INTO rooms (id, value)
                VALUES ($1, $2)
            "#,
            room.id,
            Json(room) as _,
        )
        .execute(&self.inner)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(())
    }
}
