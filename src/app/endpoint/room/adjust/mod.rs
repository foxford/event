use crate::db::adjustment::Segments;
use crate::db::room::Time;
use serde_derive::Serialize;
use serde_json::Value as JsonValue;
use svc_error::Error as SvcError;
use uuid::Uuid;

pub mod v1;
pub mod v2;

#[derive(Serialize)]
struct RoomAdjustNotification {
    room_id: Uuid,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<JsonValue>,
    #[serde(flatten)]
    result: RoomAdjustResult,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RoomAdjustResult {
    V1(RoomAdjustResultV1),
    _V2(RoomAdjustResultV2),
}

#[derive(Serialize)]
#[serde(untagged)]
enum RoomAdjustResultV1 {
    Success {
        original_room_id: Uuid,
        modified_room_id: Uuid,
        #[serde(with = "crate::db::adjustment::serde::segments")]
        modified_segments: Segments,
    },
    Error {
        error: SvcError,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum RoomAdjustResultV2 {
    _Success {
        original_room_id: Uuid,
        modified_room_id: Uuid,
        recordings: Vec<RecordingSegments>,
        #[serde(with = "crate::db::room::serde::time")]
        modified_room_time: Time,
    },
    _Error {
        error: SvcError,
    },
}

#[derive(Serialize)]
struct RecordingSegments {
    id: Uuid,
    #[serde(with = "crate::db::adjustment::serde::segments")]
    pin_segments: Segments,
    #[serde(with = "crate::db::adjustment::serde::segments")]
    modified_segments: Segments,
    #[serde(with = "crate::db::adjustment::serde::segments")]
    video_mute_segments: Segments,
    #[serde(with = "crate::db::adjustment::serde::segments")]
    audio_mute_segments: Segments,
}

impl RoomAdjustResult {
    fn status(&self) -> &'static str {
        match self {
            RoomAdjustResult::V1(RoomAdjustResultV1::Success { .. })
            | RoomAdjustResult::_V2(RoomAdjustResultV2::_Success { .. }) => "success",
            RoomAdjustResult::V1(RoomAdjustResultV1::Error { .. })
            | RoomAdjustResult::_V2(RoomAdjustResultV2::_Error { .. }) => "error",
        }
    }
}
