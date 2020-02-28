use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid::Uuid;

pub const API_VERSION: &'static str = "v1";
pub const ROOM_DURATION: i64 = 3600;

#[derive(Debug, Serialize)]
pub struct RoomCreateRequest {
    pub audience: String,
    pub time: (i64, i64),
    pub tags: JsonValue,
}

#[derive(Debug, Deserialize)]
pub struct RoomCreateResponse {
    pub id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct RoomEnterRequest {
    pub id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct RoomEnterResponse {}

#[derive(Debug, Deserialize)]
pub struct RoomEnterEvent {
    pub id: Uuid,
    pub agent_id: AgentId,
}

#[derive(Clone, Debug, Serialize)]
pub struct EventCreateRequest {
    pub room_id: Uuid,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub data: JsonValue,
}

#[derive(Debug, Deserialize)]
pub struct EventCreateResponse {}

#[derive(Debug, Serialize)]
pub struct StateReadRequest {
    pub room_id: Uuid,
    pub kind: String,
    pub sets: Vec<String>,
    pub occurred_at: i64,
    pub direction: String,
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct StateReadResponse {}
