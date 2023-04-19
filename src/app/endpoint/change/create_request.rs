use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid::Uuid;

use crate::db::change::ChangeType;

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub edition_id: Uuid,
    #[serde(flatten)]
    pub changeset: Changeset,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type", content = "event")]
pub enum Changeset {
    Addition(AdditionData),
    Modification(ModificationData),
    Removal(RemovalData),
    BulkRemoval(BulkRemovalData),
}

impl Changeset {
    pub(crate) fn as_changetype(&self) -> ChangeType {
        match self {
            Changeset::Addition(_) => ChangeType::Addition,
            Changeset::Modification(_) => ChangeType::Modification,
            Changeset::Removal(_) => ChangeType::Removal,
            Changeset::BulkRemoval(_) => ChangeType::BulkRemoval,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AdditionData {
    #[serde(rename = "type")]
    pub kind: String,
    pub data: JsonValue,
    pub set: Option<String>,
    pub label: Option<String>,
    pub occurred_at: i64,
    pub created_by: AgentId,
}

#[derive(Debug, Deserialize)]
pub struct ModificationData {
    pub event_id: Uuid,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub data: Option<JsonValue>,
    pub set: Option<String>,
    pub label: Option<String>,
    pub occurred_at: Option<i64>,
    pub created_by: Option<AgentId>,
}

#[derive(Debug, Deserialize)]
pub struct RemovalData {
    pub event_id: Uuid,
    // these fields arent really needed for Removal
    // they exists solely for frontend convenience
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub set: Option<String>,
    pub occurred_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct BulkRemovalData {
    pub set: Option<String>,
}
