use std::fmt;
use std::ops::Bound;

use chrono::{serde::ts_milliseconds, DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use uuid::Uuid;

///////////////////////////////////////////////////////////////////////////////

pub(crate) type Segment = (Bound<i64>, Bound<i64>);

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Room {
    pub(crate) id: Uuid,
}

#[derive(Debug, Serialize)]
pub(crate) struct RoomMetadata {
    #[serde(with = "ts_milliseconds")]
    pub(crate) started_at: DateTime<Utc>,
    #[serde(with = "crate::serde::milliseconds_bound_tuples")]
    pub(crate) time: Vec<Segment>,
    pub(crate) preroll: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Event {
    pub(crate) id: Uuid,
    pub(crate) data: EventData,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[serde(untagged)]
pub(crate) enum EventData {
    Stream(StreamEvent),
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct StreamEvent {
    pub(crate) cut: CutKind,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CutKind {
    Start,
    Stop,
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(super) struct CreateRoomResponse {
    pub(super) room: Room,
}

#[derive(Debug, Deserialize)]
pub(super) struct TranscodeStreamResponse {
    pub(super) room: Room,
    #[serde(with = "crate::serde::milliseconds_bound_tuples")]
    pub(super) time: Vec<Segment>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GetEventsResponse {
    pub(super) events: Vec<Event>,
    pub(super) has_next_page: bool,
    pub(super) next_page: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct CreateEventRequest {
    pub(super) data: EventData,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateEventResponse {
    pub(super) event: Event,
}

#[derive(Debug, Serialize)]
pub(super) struct DeleteEventRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reason: Option<String>,
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Ignore {}

#[derive(Debug, Deserialize)]
pub(super) struct ErrorResponse {
    pub(super) error: String,
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}
