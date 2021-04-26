use std::pin::Pin;
use std::sync::Arc;

use sqlx::postgres::PgPool as Db;
use svc_agent::AccountId;
use svc_authz::IntentObject;
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
            ["rooms", room_id, events, _, ..] if *events == "events" => {
                Some(vec!["rooms".into(), room_id.to_string(), "events".into()])
            }
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
                    if let Some(room_id) = intent.to_vec().get(1) {
                        if let Ok(room_id) = Uuid::parse_str(&room_id) {
                            if let Ok(mut conn) = db_.acquire().await {
                                let ban = crate::db::room_ban::FindQuery::new(
                                    account_id.to_owned(),
                                    room_id,
                                )
                                .execute(&mut conn)
                                .await;

                                match ban {
                                    Ok(maybe_ban) => return maybe_ban.is_some(),
                                    Err(e) => {
                                        let logger = crate::LOG.new(o!());
                                        error!(
                                            logger,
                                            "Failed to fetch ban from db, account = {}, room_id = {}, reason = {}",
                                            account_id,
                                            room_id,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                false
            }) as Pin<Box<dyn futures::Future<Output = bool> + Send>>
        },
    ) as svc_authz::BanCallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_authz_obj() {
        let obj: Box<dyn IntentObject> = AuthzObject::new(&["rooms"]).into();
        assert_eq!(obj.to_ban_key(), None);

        let obj: Box<dyn IntentObject> = AuthzObject::new(&[
            "rooms",
            "123",
            "events",
            "whatever",
            "authors",
            "foobar.usr.foxford.ru",
        ])
        .into();
        assert_eq!(
            obj.to_ban_key().map(|v| v.join("/")),
            Some("rooms/123/events".into())
        );

        let obj: Box<dyn IntentObject> = AuthzObject::new(&["rooms", "123", "events"]).into();
        assert_eq!(obj.to_ban_key(), None);
    }
}
