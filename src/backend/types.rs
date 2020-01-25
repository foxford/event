use std::fmt;

use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Room {
    pub(crate) id: Uuid,
    pub(crate) opened_at: Option<DateTime<Utc>>,
    pub(crate) closed_at: Option<DateTime<Utc>>,
    pub(crate) description: String,
    pub(crate) stream: Stream,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Stream {}

#[derive(Debug, Serialize)]
pub(super) struct CreateRoomRequest<'a> {
    pub(super) description: &'a str,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateRoomResponse {
    pub(super) room: Room,
}

#[derive(Debug, Deserialize)]
pub(super) struct ErrorResponse {
    pub(super) error: String,
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}
