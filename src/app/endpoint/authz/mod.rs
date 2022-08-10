use std::pin::Pin;
use std::sync::Arc;

use sqlx::postgres::PgPool as Db;
use svc_agent::AccountId;
use svc_authz::IntentObject;
use tracing::error;
use uuid::Uuid;

use crate::db::room::Object as Room;

#[derive(Clone)]
pub struct AuthzObject {
    object: Vec<String>,
    ban_key: Option<Vec<String>>,
}

impl AuthzObject {
    pub fn new(obj: &[&str]) -> Self {
        let ban_key = match &obj {
            ["classrooms", classroom_id, events, _, ..] if *events == "events" => Some(vec![
                "classrooms".into(),
                classroom_id.to_string(),
                "events".into(),
            ]),
            _ => None,
        };

        Self {
            object: obj.iter().map(|s| s.to_string()).collect(),
            ban_key,
        }
    }

    pub(crate) fn room(room: &Room) -> Self {
        Self {
            object: room.authz_object(),
            ban_key: None,
        }
    }
}

impl IntentObject for AuthzObject {
    fn to_ban_key(&self) -> Option<Vec<String>> {
        self.ban_key.clone()
    }

    fn to_vec(&self) -> Vec<String> {
        self.object.clone()
    }

    fn box_clone(&self) -> Box<dyn IntentObject> {
        Box::new(self.clone())
    }
}

impl From<AuthzObject> for Box<dyn IntentObject> {
    fn from(o: AuthzObject) -> Self {
        Box::new(o)
    }
}

pub fn db_ban_callback(db: Db) -> svc_authz::BanCallback {
    Arc::new(
        move |account_id: AccountId, intent: Box<dyn IntentObject>| {
            let db_ = db.clone();
            Box::pin(async move {
                if intent.to_ban_key().is_some() {
                    let intent = intent.to_vec();

                    match intent.as_slice() {
                        [obj, classroom_id, ..] if obj == "classrooms" => {
                            if let Ok(classroom_id) = Uuid::parse_str(classroom_id) {
                                if let Ok(mut conn) = db_.acquire().await {
                                    let ban = crate::db::room_ban::ClassroomFindQuery::new(
                                        account_id.to_owned(),
                                        classroom_id,
                                    )
                                    .execute(&mut conn)
                                    .await;

                                    match ban {
                                        Ok(maybe_ban) => return maybe_ban.is_some(),
                                        Err(e) => {
                                            error!(
                                            "Failed to fetch ban from db, account = {}, classroom_id = {}, reason = {}",
                                            account_id,
                                            classroom_id,
                                            e
                                        );
                                        }
                                    }
                                } else {
                                    return false;
                                }
                            } else {
                                return false;
                            }
                        }
                        _ => {
                            return false;
                        }
                    };
                }

                false
            }) as Pin<Box<dyn futures::Future<Output = bool> + Send>>
        },
    ) as svc_authz::BanCallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::prelude::*;
    use std::ops::Bound;

    #[test]
    fn create_authz_obj() {
        let obj: Box<dyn IntentObject> = AuthzObject::new(&["classrooms"]).into();
        assert_eq!(obj.to_ban_key(), None);

        let obj: Box<dyn IntentObject> = AuthzObject::new(&[
            "classrooms",
            "123",
            "events",
            "whatever",
            "authors",
            "foobar.usr.foxford.ru",
        ])
        .into();
        assert_eq!(
            obj.to_ban_key().map(|v| v.join("/")),
            Some("classrooms/123/events".into())
        );

        let obj: Box<dyn IntentObject> = AuthzObject::new(&["classrooms", "123", "events"]).into();
        assert_eq!(obj.to_ban_key(), None);
    }

    #[tokio::test]
    async fn ban_by_room_obj() {
        let db = TestDb::new().await;

        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and ban agent in the room
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_unbounded_room(&mut conn).await;

            factory::RoomBan::new(agent.agent_id(), room.id())
                .insert(&mut conn)
                .await;

            room
        };

        let banned_obj = Box::new(AuthzObject::new(&[
            "classrooms",
            &room.classroom_id().to_string(),
            "events",
            "message",
            "authors",
            "account-id.audience",
        ])) as Box<dyn IntentObject>;

        let id = Uuid::new_v4();

        let nonbanned_obj = Box::new(AuthzObject::new(&[
            "classrooms",
            &id.to_string(),
            "events",
            "message",
            "authors",
            "account-id.audience",
        ])) as Box<dyn IntentObject>;
        let cb = db_ban_callback(db.connection_pool().clone());
        let x = cb(agent.account_id().to_owned(), banned_obj.clone()).await;
        let y = cb(agent.account_id().to_owned(), nonbanned_obj).await;
        // Agent must be banned
        assert!(x);
        assert!(!y);

        let agent2 = TestAgent::new("web", "barbaz", USR_AUDIENCE);
        // This agent must not be banned
        let x = cb(agent2.account_id().to_owned(), banned_obj).await;
        assert!(!x);
    }

    #[tokio::test]
    async fn ban_by_classroom_obj() {
        let db = TestDb::new().await;

        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let classroom_id = Uuid::new_v4();

        {
            // Create room with classroom id and ban agent in the room
            let mut conn = db.get_conn().await;

            let room = factory::Room::new(classroom_id)
                .audience("foo.bar")
                .time((Bound::Unbounded, Bound::Unbounded))
                .insert(&mut conn)
                .await;

            factory::RoomBan::new(agent.agent_id(), room.id())
                .insert(&mut conn)
                .await;
        };

        let banned_obj = Box::new(AuthzObject::new(&[
            "classrooms",
            &classroom_id.to_string(),
            "events",
            "message",
            "authors",
            "account-id.audience",
        ])) as Box<dyn IntentObject>;

        let random_classroom_id = Uuid::new_v4();

        let nonbanned_obj = Box::new(AuthzObject::new(&[
            "classrooms",
            &random_classroom_id.to_string(),
            "events",
            "message",
            "authors",
            "account-id.audience",
        ])) as Box<dyn IntentObject>;
        let cb = db_ban_callback(db.connection_pool().clone());
        let x = cb(agent.account_id().to_owned(), banned_obj.clone()).await;
        let y = cb(agent.account_id().to_owned(), nonbanned_obj).await;
        // Agent must be banned
        assert!(x);
        assert!(!y);

        let agent2 = TestAgent::new("web", "barbaz", USR_AUDIENCE);
        // This agent must not be banned
        let x = cb(agent2.account_id().to_owned(), banned_obj).await;
        assert!(!x);
    }
}
