use std::fmt;

use uuid::Uuid;

pub enum Topic {
    Audience {
        audience: String,
    },
    Room {
        room_id: Uuid,
        classroom_id: Option<Uuid>,
    },
}

impl Topic {
    pub fn audience(audience: String) -> Self {
        Topic::Audience { audience }
    }

    pub fn room(room_id: Uuid, classroom_id: Option<Uuid>) -> Self {
        Topic::Room {
            room_id,
            classroom_id,
        }
    }

    pub fn nats_topic(&self) -> Option<String> {
        match &self {
            Topic::Audience { audience } => Some(format!("audiences.{}.event", audience)),
            Topic::Room { classroom_id, .. } => {
                classroom_id.map(|id| format!("classrooms.{}.event", id))
            }
        }
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Topic::Audience { audience } => write!(f, "audiences/{}/events", audience),
            Topic::Room { room_id, .. } => write!(f, "rooms/{}/events", room_id),
        }
    }
}
