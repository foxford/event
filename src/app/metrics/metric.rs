use std::hash::Hash;

use chrono::{serde::ts_seconds, DateTime, Utc};
use serde_derive::Serialize;
use svc_agent::{mqtt::MetricTags, AgentId, Authenticable};

#[derive(Serialize, Clone)]
pub(crate) struct MetricValue<T: serde::Serialize> {
    value: T,
    #[serde(with = "ts_seconds")]
    timestamp: DateTime<Utc>,
    tags: Tags,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Tags {
    Internal {
        version: String,
        agent_label: String,
        account_label: String,
        account_audience: String,
    },
    Queues {
        version: String,
        agent_label: String,
        account_label: String,
        account_audience: String,
        #[serde(flatten)]
        tags: MetricTags,
    },
    Queries {
        version: String,
        agent_label: String,
        account_label: String,
        account_audience: String,
        request_method: Option<String>,
        query_label: ProfilerKeys,
    },
}

impl Tags {
    pub fn build_internal_tags(version: &str, agent_id: &AgentId) -> Self {
        Tags::Internal {
            version: version.to_owned(),
            agent_label: agent_id.label().to_owned(),
            account_label: agent_id.as_account_id().label().to_owned(),
            account_audience: agent_id.as_account_id().audience().to_owned(),
        }
    }

    pub fn build_queues_tags(version: &str, agent_id: &AgentId, tags: MetricTags) -> Self {
        Tags::Queues {
            version: version.to_owned(),
            agent_label: agent_id.label().to_owned(),
            account_label: agent_id.as_account_id().label().to_owned(),
            account_audience: agent_id.as_account_id().audience().to_owned(),
            tags,
        }
    }

    pub fn build_queries_tags(
        version: &str,
        agent_id: &AgentId,
        query_label: ProfilerKeys,
        request_method: Option<String>,
    ) -> Self {
        Tags::Queries {
            version: version.to_owned(),
            agent_label: agent_id.label().to_owned(),
            account_label: agent_id.as_account_id().label().to_owned(),
            account_audience: agent_id.as_account_id().audience().to_owned(),
            query_label,
            request_method,
        }
    }
}

impl<T: serde::Serialize> MetricValue<T> {
    pub fn new(value: T, timestamp: DateTime<Utc>, tags: Tags) -> Self {
        Self {
            value,
            timestamp,
            tags,
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(tag = "metric")]
pub(crate) enum Metric {
    // MQTT queues.
    #[serde(rename(serialize = "apps.event.incoming_requests_total"))]
    IncomingQueueRequests(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.incoming_responses_total"))]
    IncomingQueueResponses(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.incoming_events_total"))]
    IncomingQueueEvents(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.outgoing_requests_total"))]
    OutgoingQueueRequests(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.outgoing_responses_total"))]
    OutgoingQueueResponses(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.outgoing_events_total"))]
    OutgoingQueueEvents(MetricValue<u64>),

    // DB pool.
    #[serde(rename(serialize = "apps.event.db_connections_total"))]
    DbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_db_connections_total"))]
    IdleDbConnections(MetricValue<u64>),

    // Read-only DB pool.
    #[serde(rename(serialize = "apps.event.ro_db_connections_total"))]
    RoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_ro_db_connections_total"))]
    IdleRoDbConnections(MetricValue<u64>),

    // Redis pool.
    #[serde(rename(serialize = "apps.event.redis_connections_total"))]
    RedisConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_redis_connections_total"))]
    IdleRedisConnections(MetricValue<u64>),

    // DB queries.
    #[serde(rename(serialize = "apps.event.adjustment_insert_query_p95_microseconds"))]
    AdjustmentInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.adjustment_insert_query_p99_microseconds"))]
    AdjustmentInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.adjustment_insert_query_max_microseconds"))]
    AdjustmentInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_delete_query_p95_microseconds"))]
    AgentDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_delete_query_p99_microseconds"))]
    AgentDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_delete_query_max_microseconds"))]
    AgentDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_insert_query_p95_microseconds"))]
    AgentInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_insert_query_p99_microseconds"))]
    AgentInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_insert_query_max_microseconds"))]
    AgentInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_update_query_p95_microseconds"))]
    AgentUpdateQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_update_query_p99_microseconds"))]
    AgentUpdateQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_update_query_max_microseconds"))]
    AgentUpdateQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_list_query_p95_microseconds"))]
    AgentListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_list_query_p99_microseconds"))]
    AgentListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_list_query_max_microseconds"))]
    AgentListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_delete_query_p95_microseconds"))]
    ChangeDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_delete_query_p99_microseconds"))]
    ChangeDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_delete_query_max_microseconds"))]
    ChangeDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_find_with_room_query_p95_microseconds"))]
    ChangeFindWithRoomQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_find_with_room_query_p99_microseconds"))]
    ChangeFindWithRoomQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_find_with_room_query_max_microseconds"))]
    ChangeFindWithRoomQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_insert_query_p95_microseconds"))]
    ChangeInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_insert_query_p99_microseconds"))]
    ChangeInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_insert_query_max_microseconds"))]
    ChangeInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_list_query_p95_microseconds"))]
    ChangeListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_list_query_p99_microseconds"))]
    ChangeListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.change_list_query_max_microseconds"))]
    ChangeListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_clone_events_query_p95_microseconds"))]
    EditionCloneEventsQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_clone_events_query_p99_microseconds"))]
    EditionCloneEventsQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_clone_events_query_max_microseconds"))]
    EditionCloneEventsQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_commit_txn_commit_max_p95"))]
    EditionCommitTxnCommitP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_commit_txn_commit_max_p99"))]
    EditionCommitTxnCommitP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_commit_txn_commit_max_microseconds"))]
    EditionCommitTxnCommitMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_delete_query_p95_microseconds"))]
    EditionDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_delete_query_p99_microseconds"))]
    EditionDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_delete_query_max_microseconds"))]
    EditionDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_find_with_room_query_p95_microseconds"))]
    EditionFindWithRoomQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_find_with_room_query_p99_microseconds"))]
    EditionFindWithRoomQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_find_with_room_query_max_microseconds"))]
    EditionFindWithRoomQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_insert_query_p95_microseconds"))]
    EditionInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_insert_query_p99_microseconds"))]
    EditionInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_insert_query_max_microseconds"))]
    EditionInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_list_query_p95_microseconds"))]
    EditionListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_list_query_p99_microseconds"))]
    EditionListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_list_query_max_microseconds"))]
    EditionListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_delete_query_p95_microseconds"))]
    EventDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_delete_query_p99_microseconds"))]
    EventDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_delete_query_max_microseconds"))]
    EventDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_insert_query_p95_microseconds"))]
    EventInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_insert_query_p99_microseconds"))]
    EventInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_insert_query_max_microseconds"))]
    EventInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_list_query_p95_microseconds"))]
    EventListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_list_query_p99_microseconds"))]
    EventListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_list_query_max_microseconds"))]
    EventListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_original_query_p95_microseconds"))]
    EventOriginalQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_original_query_p99_microseconds"))]
    EventOriginalQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.event_original_query_max_microseconds"))]
    EventOriginalQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_adjust_clone_events_query_p95_microseconds"))]
    RoomAdjustCloneEventsQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_adjust_clone_events_query_p99_microseconds"))]
    RoomAdjustCloneEventsQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_adjust_clone_events_query_max_microseconds"))]
    RoomAdjustCloneEventsQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_find_query_p95_microseconds"))]
    RoomFindQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_find_query_p99_microseconds"))]
    RoomFindQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_find_query_max_microseconds"))]
    RoomFindQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_insert_query_p95_microseconds"))]
    RoomInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_insert_query_p99_microseconds"))]
    RoomInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_insert_query_max_microseconds"))]
    RoomInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_update_query_p95_microseconds"))]
    RoomUpdateQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_update_query_p99_microseconds"))]
    RoomUpdateQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.room_update_query_max_microseconds"))]
    RoomUpdateQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_total_count_query_p95_microseconds"))]
    StateTotalCountQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_total_count_query_p99_microseconds"))]
    StateTotalCountQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_total_count_query_max_microseconds"))]
    StateTotalCountQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_query_p95_microseconds"))]
    StateQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_query_p99_microseconds"))]
    StateQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_query_max_microseconds"))]
    StateQueryMax(MetricValue<u64>),

    // Misc.
    #[serde(rename(serialize = "apps.event.running_requests_total"))]
    RunningRequests(MetricValue<i64>),
}

#[derive(Serialize, Clone)]
#[serde(tag = "metric")]
pub(crate) enum Metric2 {
    // MQTT queues.
    #[serde(rename(serialize = "incoming_requests_total"))]
    IncomingQueueRequests(MetricValue<u64>),
    #[serde(rename(serialize = "incoming_responses_total"))]
    IncomingQueueResponses(MetricValue<u64>),
    #[serde(rename(serialize = "incoming_events_total"))]
    IncomingQueueEvents(MetricValue<u64>),
    #[serde(rename(serialize = "outgoing_requests_total"))]
    OutgoingQueueRequests(MetricValue<u64>),
    #[serde(rename(serialize = "outgoing_responses_total"))]
    OutgoingQueueResponses(MetricValue<u64>),
    #[serde(rename(serialize = "outgoing_events_total"))]
    OutgoingQueueEvents(MetricValue<u64>),

    // DB pool.
    #[serde(rename(serialize = "db_connections_total"))]
    DbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "idle_db_connections_total"))]
    IdleDbConnections(MetricValue<u64>),

    // Read-only DB pool.
    #[serde(rename(serialize = "ro_db_connections_total"))]
    RoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "idle_ro_db_connections_total"))]
    IdleRoDbConnections(MetricValue<u64>),

    // Redis pool.
    #[serde(rename(serialize = "redis_connections_total"))]
    RedisConnections(MetricValue<u64>),
    #[serde(rename(serialize = "idle_redis_connections_total"))]
    IdleRedisConnections(MetricValue<u64>),

    // DB queries.
    #[serde(rename(serialize = "adjustment_insert_query_p95_microseconds"))]
    AdjustmentInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "adjustment_insert_query_p99_microseconds"))]
    AdjustmentInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "adjustment_insert_query_max_microseconds"))]
    AdjustmentInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "agent_delete_query_p95_microseconds"))]
    AgentDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "agent_delete_query_p99_microseconds"))]
    AgentDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "agent_delete_query_max_microseconds"))]
    AgentDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "agent_insert_query_p95_microseconds"))]
    AgentInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "agent_insert_query_p99_microseconds"))]
    AgentInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "agent_insert_query_max_microseconds"))]
    AgentInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "agent_list_query_p95_microseconds"))]
    AgentListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "agent_list_query_p99_microseconds"))]
    AgentListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "agent_list_query_max_microseconds"))]
    AgentListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "agent_update_query_p95_microseconds"))]
    AgentUpdateQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "agent_update_query_p99_microseconds"))]
    AgentUpdateQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "agent_update_query_max_microseconds"))]
    AgentUpdateQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "change_delete_query_p95_microseconds"))]
    ChangeDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "change_delete_query_p99_microseconds"))]
    ChangeDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "change_delete_query_max_microseconds"))]
    ChangeDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "change_find_with_room_query_p95_microseconds"))]
    ChangeFindWithRoomQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "change_find_with_room_query_p99_microseconds"))]
    ChangeFindWithRoomQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "change_find_with_room_query_max_microseconds"))]
    ChangeFindWithRoomQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "change_insert_query_p95_microseconds"))]
    ChangeInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "change_insert_query_p99_microseconds"))]
    ChangeInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "change_insert_query_max_microseconds"))]
    ChangeInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "change_list_query_p95_microseconds"))]
    ChangeListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "change_list_query_p99_microseconds"))]
    ChangeListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "change_list_query_max_microseconds"))]
    ChangeListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "edition_clone_events_query_p95_microseconds"))]
    EditionCloneEventsQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_clone_events_query_p99_microseconds"))]
    EditionCloneEventsQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_clone_events_query_max_microseconds"))]
    EditionCloneEventsQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "edition_commit_txn_commit_max_p95"))]
    EditionCommitTxnCommitP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_commit_txn_commit_max_p99"))]
    EditionCommitTxnCommitP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_commit_txn_commit_max_microseconds"))]
    EditionCommitTxnCommitMax(MetricValue<u64>),
    #[serde(rename(serialize = "edition_delete_query_p95_microseconds"))]
    EditionDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_delete_query_p99_microseconds"))]
    EditionDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_delete_query_max_microseconds"))]
    EditionDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "edition_find_with_room_query_p95_microseconds"))]
    EditionFindWithRoomQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_find_with_room_query_p99_microseconds"))]
    EditionFindWithRoomQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_find_with_room_query_max_microseconds"))]
    EditionFindWithRoomQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "edition_insert_query_p95_microseconds"))]
    EditionInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_insert_query_p99_microseconds"))]
    EditionInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_insert_query_max_microseconds"))]
    EditionInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "edition_list_query_p95_microseconds"))]
    EditionListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_list_query_p99_microseconds"))]
    EditionListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_list_query_max_microseconds"))]
    EditionListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "event_delete_query_p95_microseconds"))]
    EventDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "event_delete_query_p99_microseconds"))]
    EventDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "event_delete_query_max_microseconds"))]
    EventDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "event_insert_query_p95_microseconds"))]
    EventInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "event_insert_query_p99_microseconds"))]
    EventInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "event_insert_query_max_microseconds"))]
    EventInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "event_list_query_p95_microseconds"))]
    EventListQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "event_list_query_p99_microseconds"))]
    EventListQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "event_list_query_max_microseconds"))]
    EventListQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "event_original_query_p95_microseconds"))]
    EventOriginalQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "event_original_query_p99_microseconds"))]
    EventOriginalQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "event_original_query_max_microseconds"))]
    EventOriginalQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "room_adjust_clone_events_query_p95_microseconds"))]
    RoomAdjustCloneEventsQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "room_adjust_clone_events_query_p99_microseconds"))]
    RoomAdjustCloneEventsQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "room_adjust_clone_events_query_max_microseconds"))]
    RoomAdjustCloneEventsQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "room_find_query_p95_microseconds"))]
    RoomFindQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "room_find_query_p99_microseconds"))]
    RoomFindQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "room_find_query_max_microseconds"))]
    RoomFindQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "room_insert_query_p95_microseconds"))]
    RoomInsertQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "room_insert_query_p99_microseconds"))]
    RoomInsertQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "room_insert_query_max_microseconds"))]
    RoomInsertQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "room_update_query_p95_microseconds"))]
    RoomUpdateQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "room_update_query_p99_microseconds"))]
    RoomUpdateQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "room_update_query_max_microseconds"))]
    RoomUpdateQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "state_total_count_query_p95_microseconds"))]
    StateTotalCountQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "state_total_count_query_p99_microseconds"))]
    StateTotalCountQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "state_total_count_query_max_microseconds"))]
    StateTotalCountQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "state_query_p95_microseconds"))]
    StateQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "state_query_p99_microseconds"))]
    StateQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "state_query_max_microseconds"))]
    StateQueryMax(MetricValue<u64>),

    // Misc.
    #[serde(rename(serialize = "running_requests_total"))]
    RunningRequests(MetricValue<i64>),
}

impl From<Metric> for Metric2 {
    fn from(m: Metric) -> Self {
        match m {
            Metric::IncomingQueueRequests(v) => Metric2::IncomingQueueRequests(v),
            Metric::IncomingQueueResponses(v) => Metric2::IncomingQueueResponses(v),
            Metric::IncomingQueueEvents(v) => Metric2::IncomingQueueEvents(v),
            Metric::OutgoingQueueRequests(v) => Metric2::OutgoingQueueRequests(v),
            Metric::OutgoingQueueResponses(v) => Metric2::OutgoingQueueResponses(v),
            Metric::OutgoingQueueEvents(v) => Metric2::OutgoingQueueEvents(v),
            Metric::DbConnections(v) => Metric2::DbConnections(v),
            Metric::IdleDbConnections(v) => Metric2::IdleDbConnections(v),
            Metric::RoDbConnections(v) => Metric2::RoDbConnections(v),
            Metric::IdleRoDbConnections(v) => Metric2::IdleRoDbConnections(v),
            Metric::RedisConnections(v) => Metric2::RedisConnections(v),
            Metric::IdleRedisConnections(v) => Metric2::IdleRedisConnections(v),
            Metric::AdjustmentInsertQueryP95(v) => Metric2::AdjustmentInsertQueryP95(v),
            Metric::AdjustmentInsertQueryP99(v) => Metric2::AdjustmentInsertQueryP99(v),
            Metric::AdjustmentInsertQueryMax(v) => Metric2::AdjustmentInsertQueryMax(v),
            Metric::AgentDeleteQueryP95(v) => Metric2::AgentDeleteQueryP95(v),
            Metric::AgentDeleteQueryP99(v) => Metric2::AgentDeleteQueryP99(v),
            Metric::AgentDeleteQueryMax(v) => Metric2::AgentDeleteQueryMax(v),
            Metric::AgentInsertQueryP95(v) => Metric2::AgentInsertQueryP95(v),
            Metric::AgentInsertQueryP99(v) => Metric2::AgentInsertQueryP99(v),
            Metric::AgentInsertQueryMax(v) => Metric2::AgentInsertQueryMax(v),
            Metric::AgentListQueryP95(v) => Metric2::AgentListQueryP95(v),
            Metric::AgentListQueryP99(v) => Metric2::AgentListQueryP99(v),
            Metric::AgentListQueryMax(v) => Metric2::AgentListQueryMax(v),
            Metric::AgentUpdateQueryP95(v) => Metric2::AgentUpdateQueryP95(v),
            Metric::AgentUpdateQueryP99(v) => Metric2::AgentUpdateQueryP99(v),
            Metric::AgentUpdateQueryMax(v) => Metric2::AgentUpdateQueryMax(v),
            Metric::ChangeDeleteQueryP95(v) => Metric2::ChangeDeleteQueryP95(v),
            Metric::ChangeDeleteQueryP99(v) => Metric2::ChangeDeleteQueryP99(v),
            Metric::ChangeDeleteQueryMax(v) => Metric2::ChangeDeleteQueryMax(v),
            Metric::ChangeFindWithRoomQueryP95(v) => Metric2::ChangeFindWithRoomQueryP95(v),
            Metric::ChangeFindWithRoomQueryP99(v) => Metric2::ChangeFindWithRoomQueryP99(v),
            Metric::ChangeFindWithRoomQueryMax(v) => Metric2::ChangeFindWithRoomQueryMax(v),
            Metric::ChangeInsertQueryP95(v) => Metric2::ChangeInsertQueryP95(v),
            Metric::ChangeInsertQueryP99(v) => Metric2::ChangeInsertQueryP99(v),
            Metric::ChangeInsertQueryMax(v) => Metric2::ChangeInsertQueryMax(v),
            Metric::ChangeListQueryP95(v) => Metric2::ChangeListQueryP95(v),
            Metric::ChangeListQueryP99(v) => Metric2::ChangeListQueryP99(v),
            Metric::ChangeListQueryMax(v) => Metric2::ChangeListQueryMax(v),
            Metric::EditionCloneEventsQueryP95(v) => Metric2::EditionCloneEventsQueryP95(v),
            Metric::EditionCloneEventsQueryP99(v) => Metric2::EditionCloneEventsQueryP99(v),
            Metric::EditionCloneEventsQueryMax(v) => Metric2::EditionCloneEventsQueryMax(v),
            Metric::EditionCommitTxnCommitP95(v) => Metric2::EditionCommitTxnCommitP95(v),
            Metric::EditionCommitTxnCommitP99(v) => Metric2::EditionCommitTxnCommitP99(v),
            Metric::EditionCommitTxnCommitMax(v) => Metric2::EditionCommitTxnCommitMax(v),
            Metric::EditionDeleteQueryP95(v) => Metric2::EditionDeleteQueryP95(v),
            Metric::EditionDeleteQueryP99(v) => Metric2::EditionDeleteQueryP99(v),
            Metric::EditionDeleteQueryMax(v) => Metric2::EditionDeleteQueryMax(v),
            Metric::EditionFindWithRoomQueryP95(v) => Metric2::EditionFindWithRoomQueryP95(v),
            Metric::EditionFindWithRoomQueryP99(v) => Metric2::EditionFindWithRoomQueryP99(v),
            Metric::EditionFindWithRoomQueryMax(v) => Metric2::EditionFindWithRoomQueryMax(v),
            Metric::EditionInsertQueryP95(v) => Metric2::EditionInsertQueryP95(v),
            Metric::EditionInsertQueryP99(v) => Metric2::EditionInsertQueryP99(v),
            Metric::EditionInsertQueryMax(v) => Metric2::EditionInsertQueryMax(v),
            Metric::EditionListQueryP95(v) => Metric2::EditionListQueryP95(v),
            Metric::EditionListQueryP99(v) => Metric2::EditionListQueryP99(v),
            Metric::EditionListQueryMax(v) => Metric2::EditionListQueryMax(v),
            Metric::EventDeleteQueryP95(v) => Metric2::EventDeleteQueryP95(v),
            Metric::EventDeleteQueryP99(v) => Metric2::EventDeleteQueryP99(v),
            Metric::EventDeleteQueryMax(v) => Metric2::EventDeleteQueryMax(v),
            Metric::EventInsertQueryP95(v) => Metric2::EventInsertQueryP95(v),
            Metric::EventInsertQueryP99(v) => Metric2::EventInsertQueryP99(v),
            Metric::EventInsertQueryMax(v) => Metric2::EventInsertQueryMax(v),
            Metric::EventListQueryP95(v) => Metric2::EventListQueryP95(v),
            Metric::EventListQueryP99(v) => Metric2::EventListQueryP99(v),
            Metric::EventListQueryMax(v) => Metric2::EventListQueryMax(v),
            Metric::EventOriginalQueryP95(v) => Metric2::EventOriginalQueryP95(v),
            Metric::EventOriginalQueryP99(v) => Metric2::EventOriginalQueryP99(v),
            Metric::EventOriginalQueryMax(v) => Metric2::EventOriginalQueryMax(v),
            Metric::RoomAdjustCloneEventsQueryP95(v) => Metric2::RoomAdjustCloneEventsQueryP95(v),
            Metric::RoomAdjustCloneEventsQueryP99(v) => Metric2::RoomAdjustCloneEventsQueryP99(v),
            Metric::RoomAdjustCloneEventsQueryMax(v) => Metric2::RoomAdjustCloneEventsQueryMax(v),
            Metric::RoomFindQueryP95(v) => Metric2::RoomFindQueryP95(v),
            Metric::RoomFindQueryP99(v) => Metric2::RoomFindQueryP99(v),
            Metric::RoomFindQueryMax(v) => Metric2::RoomFindQueryMax(v),
            Metric::RoomInsertQueryP95(v) => Metric2::RoomInsertQueryP95(v),
            Metric::RoomInsertQueryP99(v) => Metric2::RoomInsertQueryP99(v),
            Metric::RoomInsertQueryMax(v) => Metric2::RoomInsertQueryMax(v),
            Metric::RoomUpdateQueryP95(v) => Metric2::RoomUpdateQueryP95(v),
            Metric::RoomUpdateQueryP99(v) => Metric2::RoomUpdateQueryP99(v),
            Metric::RoomUpdateQueryMax(v) => Metric2::RoomUpdateQueryMax(v),
            Metric::StateTotalCountQueryP95(v) => Metric2::StateTotalCountQueryP95(v),
            Metric::StateTotalCountQueryP99(v) => Metric2::StateTotalCountQueryP99(v),
            Metric::StateTotalCountQueryMax(v) => Metric2::StateTotalCountQueryMax(v),
            Metric::StateQueryP95(v) => Metric2::StateQueryP95(v),
            Metric::StateQueryP99(v) => Metric2::StateQueryP99(v),
            Metric::StateQueryMax(v) => Metric2::StateQueryMax(v),
            Metric::RunningRequests(v) => Metric2::RunningRequests(v),
        }
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ProfilerKeys {
    AdjustmentInsertQuery,
    AgentDeleteQuery,
    AgentInsertQuery,
    AgentListQuery,
    AgentUpdateQuery,
    ChangeDeleteQuery,
    ChangeFindWithRoomQuery,
    ChangeInsertQuery,
    ChangeListQuery,
    EditionCloneEventsQuery,
    EditionCommitTxnCommit,
    EditionDeleteQuery,
    EditionFindWithRoomQuery,
    EditionInsertQuery,
    EditionListQuery,
    EventDeleteQuery,
    EventInsertQuery,
    EventListQuery,
    EventOriginalEventQuery,
    RoomAdjustCloneEventsQuery,
    RoomFindQuery,
    RoomInsertQuery,
    RoomUpdateQuery,
    StateTotalCountQuery,
    StateQuery,
}
