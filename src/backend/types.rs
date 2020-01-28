use std::fmt;

use serde_derive::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Room {
    pub(crate) id: Uuid,
}

#[derive(Debug, Serialize)]
pub(super) struct CreateRoomRequest {}

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
