use chrono::{serde::ts_seconds, DateTime, Utc};
use serde_derive::Serialize;

#[derive(Serialize, Copy, Clone)]
pub(crate) struct MetricValue<T: serde::Serialize> {
    value: T,
    #[serde(with = "ts_seconds")]
    timestamp: DateTime<Utc>,
}

impl<T: serde::Serialize> MetricValue<T> {
    pub fn new(value: T, timestamp: DateTime<Utc>) -> Self {
        Self { value, timestamp }
    }
}

#[derive(Serialize, Copy, Clone)]
#[serde(tag = "metric")]
pub(crate) enum Metric {
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
    #[serde(rename(serialize = "apps.event.db_connections_total"))]
    DbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_db_connections_total"))]
    IdleDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.db_pool_checkin_average_total"))]
    DbPoolCheckinAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_checkin_total"))]
    MaxDbPoolCheckin(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.db_pool_checkout_average_total"))]
    DbPoolCheckoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_checkout_total"))]
    MaxDbPoolCheckout(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.db_pool_release_average_total"))]
    DbPoolReleaseAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_release_total"))]
    MaxDbPoolRelease(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.db_pool_timeout_average_total"))]
    DbPoolTimeoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_timeout_total"))]
    MaxDbPoolTimeout(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.redis_connections_total"))]
    RedisConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_redis_connections_total"))]
    IdleRedisConnections(MetricValue<u64>),
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
    #[serde(rename(serialize = "apps.event.edition_delete_query_p95_microseconds"))]
    EditionDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_delete_query_p99_microseconds"))]
    EditionDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_delete_query_max_microseconds"))]
    EditionDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_find_query_p95_microseconds"))]
    EditionFindQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_find_query_p99_microseconds"))]
    EditionFindQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.edition_find_query_max_microseconds"))]
    EditionFindQueryMax(MetricValue<u64>),
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
    #[serde(rename(serialize = "apps.event.ro_db_connections_total"))]
    RoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_ro_db_connections_total"))]
    IdleRoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_checkin_average_total"))]
    RoDbPoolCheckinAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_checkin_total"))]
    MaxRoDbPoolCheckin(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_checkout_average_total"))]
    RoDbPoolCheckoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_checkout_total"))]
    MaxRoDbPoolCheckout(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_release_average_total"))]
    RoDbPoolReleaseAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_release_total"))]
    MaxRoDbPoolRelease(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_timeout_average_total"))]
    RoDbPoolTimeoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_timeout_total"))]
    MaxRoDbPoolTimeout(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.running_requests_total"))]
    RunningRequests(MetricValue<i64>),
}

#[derive(Serialize, Copy, Clone)]
#[serde(tag = "metric")]
pub(crate) enum Metric2 {
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
    #[serde(rename(serialize = "db_connections_total"))]
    DbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "idle_db_connections_total"))]
    IdleDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "db_pool_checkin_average_total"))]
    DbPoolCheckinAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_db_pool_checkin_total"))]
    MaxDbPoolCheckin(MetricValue<u128>),
    #[serde(rename(serialize = "db_pool_checkout_average_total"))]
    DbPoolCheckoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_db_pool_checkout_total"))]
    MaxDbPoolCheckout(MetricValue<u128>),
    #[serde(rename(serialize = "db_pool_release_average_total"))]
    DbPoolReleaseAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_db_pool_release_total"))]
    MaxDbPoolRelease(MetricValue<u128>),
    #[serde(rename(serialize = "db_pool_timeout_average_total"))]
    DbPoolTimeoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_db_pool_timeout_total"))]
    MaxDbPoolTimeout(MetricValue<u128>),
    #[serde(rename(serialize = "redis_connections_total"))]
    RedisConnections(MetricValue<u64>),
    #[serde(rename(serialize = "idle_redis_connections_total"))]
    IdleRedisConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_delete_query_p95_microseconds"))]
    AgentDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_delete_query_p99_microseconds"))]
    AgentDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.agent_delete_query_max_microseconds"))]
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
    #[serde(rename(serialize = "change_delete_query_p95_microseconds"))]
    ChangeDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "change_delete_query_p99_microseconds"))]
    ChangeDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "change_delete_query_max_microseconds"))]
    ChangeDeleteQueryMax(MetricValue<u64>),
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
    #[serde(rename(serialize = "edition_delete_query_p95_microseconds"))]
    EditionDeleteQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_delete_query_p99_microseconds"))]
    EditionDeleteQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_delete_query_max_microseconds"))]
    EditionDeleteQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "edition_find_query_p95_microseconds"))]
    EditionFindQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "edition_find_query_p99_microseconds"))]
    EditionFindQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "edition_find_query_max_microseconds"))]
    EditionFindQueryMax(MetricValue<u64>),
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
    #[serde(rename(serialize = "ro_db_connections_total"))]
    RoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "idle_ro_db_connections_total"))]
    IdleRoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "ro_db_pool_checkin_average_total"))]
    RoDbPoolCheckinAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_ro_db_pool_checkin_total"))]
    MaxRoDbPoolCheckin(MetricValue<u128>),
    #[serde(rename(serialize = "ro_db_pool_checkout_average_total"))]
    RoDbPoolCheckoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_ro_db_pool_checkout_total"))]
    MaxRoDbPoolCheckout(MetricValue<u128>),
    #[serde(rename(serialize = "ro_db_pool_release_average_total"))]
    RoDbPoolReleaseAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_ro_db_pool_release_total"))]
    MaxRoDbPoolRelease(MetricValue<u128>),
    #[serde(rename(serialize = "ro_db_pool_timeout_average_total"))]
    RoDbPoolTimeoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "max_ro_db_pool_timeout_total"))]
    MaxRoDbPoolTimeout(MetricValue<u128>),
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
            Metric::DbPoolCheckinAverage(v) => Metric2::DbPoolCheckinAverage(v),
            Metric::MaxDbPoolCheckin(v) => Metric2::MaxDbPoolCheckin(v),
            Metric::DbPoolCheckoutAverage(v) => Metric2::DbPoolCheckoutAverage(v),
            Metric::MaxDbPoolCheckout(v) => Metric2::MaxDbPoolCheckout(v),
            Metric::DbPoolReleaseAverage(v) => Metric2::DbPoolReleaseAverage(v),
            Metric::MaxDbPoolRelease(v) => Metric2::MaxDbPoolRelease(v),
            Metric::DbPoolTimeoutAverage(v) => Metric2::DbPoolTimeoutAverage(v),
            Metric::MaxDbPoolTimeout(v) => Metric2::MaxDbPoolTimeout(v),
            Metric::RedisConnections(v) => Metric2::RedisConnections(v),
            Metric::IdleRedisConnections(v) => Metric2::IdleRedisConnections(v),
            Metric::AgentDeleteQueryP95(v) => Metric2::AgentDeleteQueryP95(v),
            Metric::AgentDeleteQueryP99(v) => Metric2::AgentDeleteQueryP99(v),
            Metric::AgentDeleteQueryMax(v) => Metric2::AgentDeleteQueryMax(v),
            Metric::AgentInsertQueryP95(v) => Metric2::AgentInsertQueryP95(v),
            Metric::AgentInsertQueryP99(v) => Metric2::AgentInsertQueryP99(v),
            Metric::AgentInsertQueryMax(v) => Metric2::AgentInsertQueryMax(v),
            Metric::AgentListQueryP95(v) => Metric2::AgentListQueryP95(v),
            Metric::AgentListQueryP99(v) => Metric2::AgentListQueryP99(v),
            Metric::AgentListQueryMax(v) => Metric2::AgentListQueryMax(v),
            Metric::ChangeDeleteQueryP95(v) => Metric2::ChangeDeleteQueryP95(v),
            Metric::ChangeDeleteQueryP99(v) => Metric2::ChangeDeleteQueryP99(v),
            Metric::ChangeDeleteQueryMax(v) => Metric2::ChangeDeleteQueryMax(v),
            Metric::ChangeInsertQueryP95(v) => Metric2::ChangeInsertQueryP95(v),
            Metric::ChangeInsertQueryP99(v) => Metric2::ChangeInsertQueryP99(v),
            Metric::ChangeInsertQueryMax(v) => Metric2::ChangeInsertQueryMax(v),
            Metric::ChangeListQueryP95(v) => Metric2::ChangeListQueryP95(v),
            Metric::ChangeListQueryP99(v) => Metric2::ChangeListQueryP99(v),
            Metric::ChangeListQueryMax(v) => Metric2::ChangeListQueryMax(v),
            Metric::EditionDeleteQueryP95(v) => Metric2::EditionDeleteQueryP95(v),
            Metric::EditionDeleteQueryP99(v) => Metric2::EditionDeleteQueryP99(v),
            Metric::EditionDeleteQueryMax(v) => Metric2::EditionDeleteQueryMax(v),
            Metric::EditionFindQueryP95(v) => Metric2::EditionFindQueryP95(v),
            Metric::EditionFindQueryP99(v) => Metric2::EditionFindQueryP99(v),
            Metric::EditionFindQueryMax(v) => Metric2::EditionFindQueryMax(v),
            Metric::EditionInsertQueryP95(v) => Metric2::EditionInsertQueryP95(v),
            Metric::EditionInsertQueryP99(v) => Metric2::EditionInsertQueryP99(v),
            Metric::EditionInsertQueryMax(v) => Metric2::EditionInsertQueryMax(v),
            Metric::EditionListQueryP95(v) => Metric2::EditionListQueryP95(v),
            Metric::EditionListQueryP99(v) => Metric2::EditionListQueryP99(v),
            Metric::EditionListQueryMax(v) => Metric2::EditionListQueryMax(v),
            Metric::EventInsertQueryP95(v) => Metric2::EventInsertQueryP95(v),
            Metric::EventInsertQueryP99(v) => Metric2::EventInsertQueryP99(v),
            Metric::EventInsertQueryMax(v) => Metric2::EventInsertQueryMax(v),
            Metric::EventListQueryP95(v) => Metric2::EventListQueryP95(v),
            Metric::EventListQueryP99(v) => Metric2::EventListQueryP99(v),
            Metric::EventListQueryMax(v) => Metric2::EventListQueryMax(v),
            Metric::EventOriginalQueryP95(v) => Metric2::EventOriginalQueryP95(v),
            Metric::EventOriginalQueryP99(v) => Metric2::EventOriginalQueryP99(v),
            Metric::EventOriginalQueryMax(v) => Metric2::EventOriginalQueryMax(v),
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
            Metric::RoDbConnections(v) => Metric2::RoDbConnections(v),
            Metric::IdleRoDbConnections(v) => Metric2::IdleRoDbConnections(v),
            Metric::RoDbPoolCheckinAverage(v) => Metric2::RoDbPoolCheckinAverage(v),
            Metric::MaxRoDbPoolCheckin(v) => Metric2::MaxRoDbPoolCheckin(v),
            Metric::RoDbPoolCheckoutAverage(v) => Metric2::RoDbPoolCheckoutAverage(v),
            Metric::MaxRoDbPoolCheckout(v) => Metric2::MaxRoDbPoolCheckout(v),
            Metric::RoDbPoolReleaseAverage(v) => Metric2::RoDbPoolReleaseAverage(v),
            Metric::MaxRoDbPoolRelease(v) => Metric2::MaxRoDbPoolRelease(v),
            Metric::RoDbPoolTimeoutAverage(v) => Metric2::RoDbPoolTimeoutAverage(v),
            Metric::MaxRoDbPoolTimeout(v) => Metric2::MaxRoDbPoolTimeout(v),
            Metric::RunningRequests(v) => Metric2::RunningRequests(v),
        }
    }
}
