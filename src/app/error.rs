use enum_iterator::IntoEnumIterator;
use std::error::Error as StdError;
use std::fmt;

use svc_agent::mqtt::ResponseStatus;
use svc_error::{extension::sentry, Error as SvcError};
use tracing::warn;

////////////////////////////////////////////////////////////////////////////////

struct ErrorKindProperties {
    status: ResponseStatus,
    kind: &'static str,
    title: &'static str,
    is_notify_sentry: bool,
}

#[derive(Debug, Clone, Copy, IntoEnumIterator, Hash, PartialEq, Eq)]
pub enum ErrorKind {
    AccessDenied,
    AgentNotEnteredTheRoom,
    AuthorizationFailed,
    BrokerRequestFailed,
    ChangeNotFound,
    DbConnAcquisitionFailed,
    DbQueryFailed,
    EditionCommitTaskFailed,
    EditionNotFound,
    InvalidPayload,
    InvalidQueryString,
    InvalidRoomTime,
    InvalidStateSets,
    InvalidSubscriptionObject,
    MessageHandlingFailed,
    NoS3Client,
    StatsCollectionFailed,
    PublishFailed,
    RoomAdjustTaskFailed,
    RoomClosed,
    RoomNotFound,
    SerializationFailed,
    TransientEventCreationFailed,
    UnknownMethod,
}

impl ErrorKind {
    pub fn status(self) -> ResponseStatus {
        let properties: ErrorKindProperties = self.into();
        properties.status
    }

    pub fn kind(self) -> &'static str {
        let properties: ErrorKindProperties = self.into();
        properties.kind
    }

    pub fn title(self) -> &'static str {
        let properties: ErrorKindProperties = self.into();
        properties.title
    }

    pub fn is_notify_sentry(self) -> bool {
        let properties: ErrorKindProperties = self.into();
        properties.is_notify_sentry
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let properties: ErrorKindProperties = self.to_owned().into();
        write!(f, "{}", properties.title)
    }
}

impl From<ErrorKind> for ErrorKindProperties {
    fn from(val: ErrorKind) -> Self {
        match val {
            ErrorKind::AccessDenied => ErrorKindProperties {
                status: ResponseStatus::FORBIDDEN,
                kind: "access_denied",
                title: "Access denied",
                is_notify_sentry: false,
            },
            ErrorKind::AgentNotEnteredTheRoom => ErrorKindProperties {
                status: ResponseStatus::NOT_FOUND,
                kind: "agent_not_entered_the_room",
                title: "Agent not entered the room",
                is_notify_sentry: false,
            },
            ErrorKind::AuthorizationFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "authorization_failed",
                title: "Authorization failed",
                is_notify_sentry: false,
            },
            ErrorKind::BrokerRequestFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "broker_request_failed",
                title: "Broker request failed",
                is_notify_sentry: true,
            },
            ErrorKind::ChangeNotFound => ErrorKindProperties {
                status: ResponseStatus::NOT_FOUND,
                kind: "change_not_found",
                title: "Change not found",
                is_notify_sentry: false,
            },
            ErrorKind::DbConnAcquisitionFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "database_connection_acquisition_failed",
                title: "Database connection acquisition failed",
                is_notify_sentry: true,
            },
            ErrorKind::DbQueryFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "database_query_failed",
                title: "Database query failed",
                is_notify_sentry: true,
            },
            ErrorKind::EditionCommitTaskFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "edition_commit_task_failed",
                title: "Edition commit task failed",
                is_notify_sentry: true,
            },
            ErrorKind::EditionNotFound => ErrorKindProperties {
                status: ResponseStatus::NOT_FOUND,
                kind: "edition_not_found",
                title: "Edition not found",
                is_notify_sentry: false,
            },
            ErrorKind::InvalidPayload => ErrorKindProperties {
                status: ResponseStatus::BAD_REQUEST,
                kind: "invalid_payload",
                title: "Invalid payload",
                is_notify_sentry: false,
            },
            ErrorKind::InvalidQueryString => ErrorKindProperties {
                status: ResponseStatus::BAD_REQUEST,
                kind: "invalid_query_string",
                title: "Invalid query string",
                is_notify_sentry: false,
            },
            ErrorKind::InvalidRoomTime => ErrorKindProperties {
                status: ResponseStatus::BAD_REQUEST,
                kind: "invalid_room_time",
                title: "Invalid room time",
                is_notify_sentry: false,
            },
            ErrorKind::InvalidStateSets => ErrorKindProperties {
                status: ResponseStatus::BAD_REQUEST,
                kind: "invalid_state_sets",
                title: "Invalid state sets",
                is_notify_sentry: false,
            },
            ErrorKind::InvalidSubscriptionObject => ErrorKindProperties {
                status: ResponseStatus::BAD_REQUEST,
                kind: "invalid_subscription_object",
                title: "Invalid subscription object",
                is_notify_sentry: true,
            },
            ErrorKind::MessageHandlingFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "message_handling_failed",
                title: "Message handling failed",
                is_notify_sentry: true,
            },
            ErrorKind::NoS3Client => ErrorKindProperties {
                status: ResponseStatus::NOT_IMPLEMENTED,
                kind: "no_s3_client",
                title: "No s3 configuration, nowhere to dump events to",
                is_notify_sentry: true,
            },
            ErrorKind::SerializationFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "serialization_failed",
                title: "Serialization failed",
                is_notify_sentry: true,
            },
            ErrorKind::StatsCollectionFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "stats_collection_failed",
                title: "Stats collection failed",
                is_notify_sentry: true,
            },
            ErrorKind::PublishFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "publish_failed",
                title: "Publish failed",
                is_notify_sentry: true,
            },
            ErrorKind::RoomAdjustTaskFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "room_adjust_task_failed",
                title: "Room adjust task failed",
                is_notify_sentry: true,
            },
            ErrorKind::RoomClosed => ErrorKindProperties {
                status: ResponseStatus::NOT_FOUND,
                kind: "room_closed",
                title: "Room closed",
                is_notify_sentry: false,
            },
            ErrorKind::RoomNotFound => ErrorKindProperties {
                status: ResponseStatus::NOT_FOUND,
                kind: "room_not_found",
                title: "Room not found",
                is_notify_sentry: false,
            },
            ErrorKind::TransientEventCreationFailed => ErrorKindProperties {
                status: ResponseStatus::UNPROCESSABLE_ENTITY,
                kind: "transient_event_creation_failed",
                title: "Transient event creation failed",
                is_notify_sentry: true,
            },
            ErrorKind::UnknownMethod => ErrorKindProperties {
                status: ResponseStatus::METHOD_NOT_ALLOWED,
                kind: "unknown_method",
                title: "Unknown method",
                is_notify_sentry: false,
            },
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

use std::collections::HashMap;

pub struct Error {
    kind: ErrorKind,
    source: Box<dyn AsRef<dyn StdError + Send + Sync + 'static> + Send + 'static>,
    tags: HashMap<String, String>,
}

impl Error {
    pub(crate) fn new<E>(kind: ErrorKind, source: E) -> Self
    where
        E: AsRef<dyn StdError + Send + Sync + 'static> + Send + 'static,
    {
        Self {
            kind,
            source: Box::new(source),
            tags: HashMap::new(),
        }
    }

    pub fn status(&self) -> ResponseStatus {
        self.kind.status()
    }

    pub fn kind(&self) -> &str {
        self.kind.kind()
    }

    pub fn title(&self) -> &str {
        self.kind.title()
    }

    pub fn error_kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn source(&self) -> &(dyn StdError + Send + Sync + 'static) {
        self.source.as_ref().as_ref()
    }

    pub fn tag(&mut self, k: &str, v: &str) {
        self.tags.insert(k.to_owned(), v.to_owned());
    }

    pub fn to_svc_error(&self) -> SvcError {
        let properties: ErrorKindProperties = self.kind.into();

        let mut e = SvcError::builder()
            .status(properties.status)
            .kind(properties.kind, properties.title)
            .detail(&self.source().to_string())
            .build();

        for (tag, val) in self.tags.iter() {
            e.set_extra(tag, val);
        }
        e
    }

    pub fn notify_sentry(&self) {
        if !self.kind.is_notify_sentry() {
            return;
        }

        sentry::send(self.to_svc_error()).unwrap_or_else(|err| {
            warn!("Error sending error to Sentry: {:?}", err);
        });
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("source", &self.source())
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

impl From<svc_authz::Error> for Error {
    fn from(source: svc_authz::Error) -> Self {
        let kind = match source.kind() {
            svc_authz::ErrorKind::Forbidden(_) => ErrorKind::AccessDenied,
            _ => ErrorKind::AuthorizationFailed,
        };

        Self {
            kind,
            source: Box::new(anyhow::Error::from(source)),
            tags: HashMap::new(),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

pub trait ErrorExt<T> {
    fn error(self, kind: ErrorKind) -> Result<T, Error>;
}

impl<T, E: Into<anyhow::Error>> ErrorExt<T> for Result<T, E> {
    fn error(self, kind: ErrorKind) -> Result<T, Error> {
        self.map_err(|source| Error::new(kind, source.into()))
    }
}
