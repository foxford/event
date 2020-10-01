use std::error::Error as StdError;
use std::fmt;

use svc_agent::mqtt::ResponseStatus;
use svc_error::Error as SvcError;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone, Copy)]
pub(crate) enum ErrorKind {
    AccessDenied,
    AgentNotEnteredTheRoom,
    AuthorizationFailed,
    ChangeNotFound,
    DbConnAcquisitionFailed,
    DbQueryFailed,
    EditionCommitTaskFailed,
    EditionNotFound,
    InvalidRoomTime,
    InvalidStateSets,
    InvalidSubscriptionObject,
    MessageHandlingFailed,
    StatsCollectionFailed,
    PublishFailed,
    RoomAdjustTaskFailed,
    RoomNotFound,
    SerializationFailed,
    TransientEventCreationFailed,
}

impl Into<(ResponseStatus, &'static str, &'static str)> for ErrorKind {
    fn into(self) -> (ResponseStatus, &'static str, &'static str) {
        match self {
            Self::AccessDenied => (ResponseStatus::FORBIDDEN, "access_denied", "Access denied"),
            Self::AgentNotEnteredTheRoom => (
                ResponseStatus::NOT_FOUND,
                "agent_not_entered_the_room",
                "Agent not entered the room",
            ),
            Self::AuthorizationFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "authorization_failed",
                "Authorization failed",
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
            Self::MessageHandlingFailed => (
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "message_handling_failed",
                "Message handling failed",
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

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (_status, _kind, title) = self.to_owned().into();
        write!(f, "{}", title)
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct Error {
    kind: ErrorKind,
    source: Box<dyn AsRef<dyn StdError + Send + Sync + 'static>>,
}

impl Error {
    pub(crate) fn new<E>(kind: ErrorKind, source: E) -> Self
    where
        E: AsRef<dyn StdError + Send + Sync + 'static> + 'static,
    {
        Self {
            kind,
            source: Box::new(source),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("source", &self.source.as_ref().as_ref())
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.source.as_ref().as_ref())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref().as_ref())
    }
}

impl Into<SvcError> for Error {
    fn into(self) -> SvcError {
        let (status, kind, title) = self.kind.into();

        SvcError::builder()
            .status(status)
            .kind(kind, title)
            .detail(&self.source.as_ref().as_ref().to_string())
            .build()
    }
}

impl From<svc_authz::Error> for Error {
    fn from(source: svc_authz::Error) -> Self {
        let kind = match source.kind() {
            svc_authz::ErrorKind::Forbidden(_) => ErrorKind::AccessDenied,
            _ => ErrorKind::AuthorizationFailed,
        };

        Self {
            kind,
            source: Box::new(anyhow::Error::from(source)),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) trait ErrorExt<T> {
    fn error(self, kind: ErrorKind) -> Result<T, Error>;
}

impl<T, E: AsRef<dyn StdError + Send + Sync + 'static> + 'static> ErrorExt<T> for Result<T, E> {
    fn error(self, kind: ErrorKind) -> Result<T, Error> {
        self.map_err(|source| Error::new(kind, source))
    }
}
