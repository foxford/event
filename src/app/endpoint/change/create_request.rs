use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid08::Uuid;

use crate::db::change::ChangeType;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    pub(crate) edition_id: Uuid,
    #[serde(flatten)]
    pub(crate) changeset: Changeset,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type", content = "event")]
pub(crate) enum Changeset {
    Addition(AdditionData),
    Modification(ModificationData),
    Removal(RemovalData),
}

impl Changeset {
    pub(crate) fn as_changetype(&self) -> ChangeType {
        match self {
            Changeset::Addition(_) => ChangeType::Addition,
            Changeset::Modification(_) => ChangeType::Modification,
            Changeset::Removal(_) => ChangeType::Removal,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdditionData {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) data: JsonValue,
    pub(crate) set: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) occurred_at: i64,
    pub(crate) created_by: AgentId,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModificationData {
    pub(crate) event_id: Uuid,
    #[serde(rename = "type")]
    pub(crate) kind: Option<String>,
    pub(crate) data: Option<JsonValue>,
    pub(crate) set: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) occurred_at: Option<i64>,
    pub(crate) created_by: Option<AgentId>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemovalData {
    pub(crate) event_id: Uuid,
    // these fields arent really needed for Removal
    // they exists solely for frontend convenience
    #[serde(rename = "type")]
    pub(crate) kind: Option<String>,
    pub(crate) set: Option<String>,
    pub(crate) occurred_at: Option<i64>,
}
