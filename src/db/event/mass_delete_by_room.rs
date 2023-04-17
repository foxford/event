use sqlx::postgres::PgConnection;
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct MassDeleteQuery<'a> {
    room_id: Uuid,
    set: Option<&'a str>,
    edition_id: Option<Uuid>,
}

impl<'a> MassDeleteQuery<'a> {
    pub(crate) fn new(room_id: Uuid, set: Option<&'a str>, edition_id: Option<Uuid>) -> Self {
        Self { room_id, set, edition_id }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<()> {
        match (self.set, self.edition_id) {
            (None, None) => {
                sqlx::query!(
                    "
                    UPDATE event
                    SET deleted_at = NOW()
                    WHERE deleted_at is null
                    AND   room_id = $1
                    ",
                    self.room_id
                )
            }
            (Some(set), None) => {
                sqlx::query!(
                    "
                    UPDATE event
                    SET deleted_at = NOW()
                    WHERE deleted_at is null
                    AND   room_id = $1
                    AND   set = $2
                    ",
                    self.room_id,
                    set,
                )
            }
            (None, Some(edition_id)) => {
                sqlx::query!(
                    "
                    UPDATE event
                    SET deleted_at = NOW()
                    FROM change c
                    WHERE deleted_at is null
                    AND   room_id = $1
                    AND   c.edition_id = $2
                    ",
                    self.room_id,
                    edition_id
                )
            }
            (Some(set), Some(edition_id)) => {
                sqlx::query!(
                    "
                    UPDATE event
                    SET deleted_at = NOW()
                    FROM change c
                    WHERE deleted_at is null
                    AND   room_id = $1
                    AND   set = $2
                    AND   c.edition_id = $3
                    ",
                    self.room_id,
                    set,
                    edition_id
                )
            }
        }
        .execute(conn)
        .await
        .map(|_| ())
    }
}

