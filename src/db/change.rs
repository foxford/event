use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::edition::Object as Edition;
use crate::schema::change;

#[derive(DbEnum, Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[PgType = "change_type"]
#[DieselType = "Change_type"]
pub(crate) enum ChangeType {
    Addition,
    Modification,
    Removal,
}

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, QueryableByName, Associations)]
#[belongs_to(Edition, foreign_key = "edition_id")]
#[table_name = "change"]
pub(crate) struct Object {
    id: Uuid,
    edition_id: Uuid,
    #[serde(rename = "type")]
    kind: ChangeType,
    data: JsonValue,
    event_id: Option<Uuid>,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}
