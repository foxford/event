use std::fmt::Display;

use svc_agent::mqtt::ResponseStatus;
use svc_error::Error as SvcError;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone, Copy)]
pub(crate) enum AppError {
    AccessDenied,
    AgentNotEnteredTheRoom,
    ChangeNotFound,
    DbConnAcquisitionFailed,
    DbQueryFailed,
    EditionCommitTaskFailed,
    EditionNotFound,
    InvalidRoomTime,
    InvalidStateSets,
    InvalidSubscriptionObject,
    StatsCollectionFailed,
    PublishFailed,
    RoomAdjustTaskFailed,
    RoomNotFound,
    SerializationFailed,
    TransientEventCreationFailed,
}

impl Into<(ResponseStatus, &'static str, &'static str)> for AppError {
    fn into(self) -> (ResponseStatus, &'static str, &'static str) {
        match self {
            Self::AccessDenied => (ResponseStatus::FORBIDDEN, "access_denied", "Access denied"),
            Self::AgentNotEnteredTheRoom => (
                ResponseStatus::NOT_FOUND,
                "agent_not_entered_the_room",
                "Agent not entered the room",
            ),
            Self::ChangeNotFound => (
                ResponseStatus::NOT_FOUND,
                "change_not_found",
                "Change not found",
            ),
            Self::DbConnAcquisitionFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "database_connection_acquisition_failed",
                "Database connection acquisition failed",
            ),
            Self::DbQueryFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "database_query_failed",
                "Database query failed",
            ),
            Self::EditionCommitTaskFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "edition_commit_task_failed",
                "Edition commit task failed",
            ),
            Self::EditionNotFound => (
                ResponseStatus::NOT_FOUND,
                "edition_not_found",
                "Edition not found",
            ),
            Self::InvalidRoomTime => (
                ResponseStatus::BAD_REQUEST,
                "invalid_room_time",
                "Invalid room time",
            ),
            Self::InvalidStateSets => (
                ResponseStatus::BAD_REQUEST,
                "invalid_state_sets",
                "Invalid state sets",
            ),
            Self::InvalidSubscriptionObject => (
                ResponseStatus::BAD_REQUEST,
                "invalid_subscription_object",
                "Invalid subscription object",
            ),
            Self::SerializationFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "serialization_failed",
                "Serialization failed",
            ),
            Self::StatsCollectionFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "stats_collection_failed",
                "Stats collection failed",
            ),
            Self::PublishFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "publish_failed",
                "Publish failed",
            ),
            Self::RoomAdjustTaskFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "room_adjust_task_failed",
                "Room adjust task failed",
            ),
            Self::RoomNotFound => (
                ResponseStatus::NOT_FOUND,
                "room_not_found",
                "Room not found",
            ),
            Self::TransientEventCreationFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "transient_event_creation_failed",
                "Transient event creation failed",
            ),
        }
    }
}

impl AppError {
    pub(crate) fn into_svc_error(self, detail: impl Display) -> SvcError {
        let (status, kind, title) = self.into();

        SvcError::builder()
            .status(status)
            .kind(kind, title)
            .detail(&format!("{}", detail))
            .build()
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) trait ErrorExt<T> {
    fn error(self, app_err: AppError) -> Result<T, SvcError>;
}

impl<T, E: Display> ErrorExt<T> for Result<T, E> {
    fn error(self, app_err: AppError) -> Result<T, SvcError> {
        self.map_err(|err| app_err.into_svc_error(err))
    }
}
