use anyhow::Context as AnyhowContext;
use async_std::stream;
use async_trait::async_trait;
use serde_derive::{Deserialize, Serialize};
use serde_json::json;
use svc_agent::mqtt::{IncomingRequestProperties, ResponseStatus};
use svc_agent::AccountId;
use svc_authn::Authenticable;
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;
use crate::db::room_ban::{DeleteQuery as BanDeleteQuery, InsertQuery as BanInsertQuery};

///////////////////////////////////////////////////////////////////////////////

const MAX_LIMIT: usize = 25;

#[derive(Debug, Deserialize)]
pub(crate) struct ListRequest {
    room_id: Uuid,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub(crate) struct ListHandler;

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let room = helpers::find_room(
            context,
            payload.room_id,
            helpers::RoomTimeRequirement::Open,
            reqp.method(),
        )
        .await?;

        // Authorize agents listing in the room.
        let object = {
            let object = room.authz_object();
            let object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            AuthzObject::new(&object).into()
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "read".into(),
            )
            .await?;

        // Get agents list in the room.
        let agents = {
            let mut conn = context.get_ro_conn().await?;

            let query = db::agent::ListWithBansQuery::new(
                payload.room_id,
                db::agent::Status::Ready,
                payload.offset.unwrap_or(0),
                std::cmp::min(payload.limit.unwrap_or(MAX_LIMIT), MAX_LIMIT),
            );

            context
                .profiler()
                .measure(
                    (ProfilerKeys::AgentListQuery, Some(reqp.method().to_owned())),
                    query.execute(&mut conn),
                )
                .await
                .context("Failed to list agents")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond with agents list.
        Ok(Box::new(stream::once(helpers::build_response(
            ResponseStatus::OK,
            agents,
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        ))))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateRequest {
    account_id: AccountId,
    room_id: Uuid,
    value: bool,
    reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BanNotification {
    account_id: AccountId,
    banned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct TenantBanNotification {
    room_id: Uuid,
    account_id: AccountId,
    banned_by: AccountId,
    banned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    classroom_id: Option<Uuid>,
}

pub(crate) struct UpdateHandler;

#[async_trait]
impl RequestHandler for UpdateHandler {
    type Payload = UpdateRequest;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let room = helpers::find_room(
            context,
            payload.room_id,
            helpers::RoomTimeRequirement::Open,
            reqp.method(),
        )
        .await?;

        helpers::add_room_logger_tags(context, &room);

        let author = reqp.as_account_id().to_string();

        let object = {
            let object = room.authz_object();
            let mut object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            object.extend(["claims", "role", "authors", &author].iter());
            AuthzObject::new(&object)
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.to_owned(),
                object.into(),
                "create".into(),
            )
            .await?;

        let object = {
            let object = room.authz_object();
            let mut object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            object.push("events");
            AuthzObject::new(&object)
        };

        if payload.value {
            let mut query = BanInsertQuery::new(payload.account_id.clone(), payload.room_id);

            if let Some(ref reason) = payload.reason {
                query.reason(reason);
            }

            let mut conn = context.get_conn().await?;
            context
                .profiler()
                .measure(
                    (ProfilerKeys::BanInsertQuery, Some(reqp.method().to_owned())),
                    query.execute(&mut conn),
                )
                .await
                .context("Failed to insert room ban")
                .error(AppErrorKind::DbQueryFailed)?;
        } else {
            let query = BanDeleteQuery::new(payload.account_id.clone(), payload.room_id);

            let mut conn = context.get_conn().await?;
            context
                .profiler()
                .measure(
                    (ProfilerKeys::BanDeleteQuery, Some(reqp.method().to_owned())),
                    query.execute(&mut conn),
                )
                .await
                .context("Failed to delete room ban")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        if let Err(e) = context
            .authz()
            .ban(
                room.audience().into(),
                payload.account_id.clone(),
                object.into(),
                payload.value,
                context.config().ban_duration() as usize,
            )
            .await
        {
            error!(
                context.logger(),
                "Failed to write account ban into redis, account = {}, ban = {}, reason = {}",
                &author,
                payload.value,
                e
            );
        }

        let mut messages = Vec::with_capacity(3);

        // Respond to the agent.
        messages.push(helpers::build_response(
            ResponseStatus::OK,
            json!({}),
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        ));

        let tenant_notification = TenantBanNotification {
            room_id: room.id(),
            account_id: payload.account_id.clone(),
            reason: payload.reason.clone(),
            banned_by: reqp.to_owned().as_account_id().to_owned(),
            banned: payload.value,
            classroom_id: room.classroom_id(),
        };

        messages.push(helpers::build_notification(
            "agent.ban",
            &format!("audiences/{}/events", room.audience()),
            tenant_notification,
            reqp,
            context.start_timestamp(),
        ));

        let room_notification = BanNotification {
            account_id: payload.account_id,
            banned: payload.value,
            reason: payload.reason,
        };

        // Notify room subscribers.
        messages.push(helpers::build_notification(
            "agent.update",
            &format!("rooms/{}/events", room.id()),
            room_notification,
            reqp,
            context.start_timestamp(),
        ));

        Ok(Box::new(stream::from_iter(messages)))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use serde_derive::Deserialize;
    use svc_agent::AgentId;
    use uuid::Uuid;

    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[derive(Deserialize)]
    struct MaybeBannedAgent {
        agent_id: AgentId,
        room_id: Uuid,
        banned: Option<bool>,
    }

    #[test]
    fn list_agents() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let banned_agent = TestAgent::new("web", "user456", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                shared_helpers::insert_agent(&mut conn, banned_agent.agent_id(), room.id()).await;
                BanInsertQuery::new(banned_agent.account_id().to_owned(), room.id())
                    .execute(&mut conn)
                    .await
                    .expect("Failed to insert ban");

                room
            };

            // Allow agent to list agents in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();

            authz.allow(agent.account_id(), vec!["rooms", &room_id], "read");

            // Make agent.list request.
            let mut context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                offset: None,
                limit: None,
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Agents listing failed");

            // Assert response.
            let (agents, respp, _) = find_response::<Vec<MaybeBannedAgent>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(agents.len(), 2);
            assert_eq!(&agents[1].agent_id, agent.agent_id());
            assert_eq!(agents[1].room_id, room.id());
            assert_eq!(agents[1].banned, Some(false));

            assert_eq!(&agents[0].agent_id, banned_agent.agent_id());
            assert_eq!(agents[0].room_id, room.id());
            assert_eq!(agents[0].banned, Some(true));
        });
    }

    #[test]
    fn list_agents_not_authorized() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new().await;

            let room = {
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = ListRequest {
                room_id: room.id(),
                offset: None,
                limit: None,
            };

            let err = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on agents listing");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        });
    }

    #[test]
    fn list_agents_closed_room() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new().await;

            let room = {
                // Create closed room.
                let mut conn = db.get_conn().await;
                shared_helpers::insert_closed_room(&mut conn).await
            };

            // Allow agent to list agents in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();

            authz.allow(agent.account_id(), vec!["rooms", &room_id], "read");

            // Make agent.list request.
            let mut context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                offset: None,
                limit: None,
            };

            let err = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on agents listing");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_closed");
        });
    }

    #[test]
    fn list_agents_missing_room() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

            let payload = ListRequest {
                room_id: Uuid::new_v4(),
                offset: None,
                limit: None,
            };

            let err = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on agents listing");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        });
    }

    #[test]
    fn ban_agent() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let user = TestAgent::new("web", "user", USR_AUDIENCE);
            let admin = TestAgent::new("web", "admin", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, user.agent_id(), room.id()).await;
                shared_helpers::insert_agent(&mut conn, admin.agent_id(), room.id()).await;
                room
            };

            let is_banned_f = test_db_ban_callback(db.clone());

            // Allow agent to list agents in the room.
            let mut authz = DbBanTestAuthz::new(is_banned_f);
            let room_id = room.id().to_string();

            authz.allow(
                admin.account_id(),
                vec![
                    "rooms",
                    &room_id,
                    "claims",
                    "role",
                    "authors",
                    &admin.account_id().to_string(),
                ],
                "create",
            );

            authz.allow(
                user.account_id(),
                vec![
                    "rooms",
                    &room_id,
                    "events",
                    "message",
                    "authors",
                    &user.account_id().to_string(),
                ],
                "create",
            );

            let mut context = TestContext::new_with_ban(db, authz);

            // User posts something bad and is allowed to do so
            let payload = super::super::event::CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "banmsg" }),
                is_claim: false,
                is_persistent: true,
            };

            let messages = handle_request::<crate::app::endpoint::event::CreateHandler>(
                &mut context,
                &user,
                payload,
            )
            .await
            .expect("Event creation failed");

            assert_eq!(messages.len(), 2);

            // Assert response.
            let (_, respp, _) = find_response::<crate::db::event::Object>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);

            // Admin bans user
            let payload = UpdateRequest {
                room_id: room.id(),
                account_id: user.account_id().to_owned(),
                value: true,
                reason: Some("some reason".into()),
            };

            let messages = handle_request::<UpdateHandler>(&mut context, &admin, payload)
                .await
                .expect("Agent ban failed");

            // Assert response.
            let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);

            let (ev_body, evp, _) =
                find_event_by_predicate::<BanNotification, _>(messages.as_slice(), |evp| {
                    evp.label() == "agent.update"
                })
                .expect("Failed to find agent.update event");
            assert_eq!(evp.label(), "agent.update");
            assert_eq!(ev_body.account_id, *user.account_id());
            assert_eq!(ev_body.banned, true);

            let (ev_body, evp, _) =
                find_event_by_predicate::<TenantBanNotification, _>(messages.as_slice(), |evp| {
                    evp.label() == "agent.ban"
                })
                .expect("Failed to find agent.ban event");
            assert_eq!(evp.label(), "agent.ban");
            assert_eq!(ev_body.account_id, *user.account_id());
            assert_eq!(ev_body.room_id, room.id());
            assert_eq!(ev_body.banned, true);
            assert_eq!(ev_body.banned_by, *admin.account_id());

            let mut conn = context.db().acquire().await.expect("Failed conn checkout");
            let db_ban = db::room_ban::FindQuery::new(ev_body.account_id, room.id())
                .execute(&mut conn)
                .await
                .expect("Failed to query ban in db")
                .expect("Missing ban in db");

            assert_eq!(db_ban.account_id(), user.account_id());
            assert_eq!(*db_ban.room_id(), room.id());
            assert_eq!(db_ban.reason(), Some("some reason"));

            drop(conn);

            // Ban once again, this should do nothing
            let payload = UpdateRequest {
                room_id: room.id(),
                account_id: user.account_id().to_owned(),
                value: true,
                reason: None,
            };

            let messages = handle_request::<UpdateHandler>(&mut context, &admin, payload)
                .await
                .expect("Agent ban failed");

            // Assert response.
            let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);

            // Make event.create request to check that user is actually banned
            let payload = super::super::event::CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
            };

            let err =
                handle_request::<super::super::event::CreateHandler>(&mut context, &user, payload)
                    .await
                    .expect_err("Unexpected success on event creation");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);

            // Unban the user
            let payload = UpdateRequest {
                room_id: room.id(),
                account_id: user.account_id().to_owned(),
                value: false,
                reason: None,
            };

            let messages = handle_request::<UpdateHandler>(&mut context, &admin, payload)
                .await
                .expect("Agent ban failed");

            // Assert response.
            let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);

            let (ev_body, evp, _) =
                find_event_by_predicate::<BanNotification, _>(messages.as_slice(), |evp| {
                    evp.label() == "agent.update"
                })
                .expect("Failed to find agent.update event");
            assert_eq!(evp.label(), "agent.update");
            assert_eq!(ev_body.account_id, *user.account_id());
            assert_eq!(ev_body.banned, false);

            let (ev_body, evp, _) =
                find_event_by_predicate::<TenantBanNotification, _>(messages.as_slice(), |evp| {
                    evp.label() == "agent.ban"
                })
                .expect("Failed to find agent.ban event");
            assert_eq!(evp.label(), "agent.ban");
            assert_eq!(ev_body.account_id, *user.account_id());
            assert_eq!(ev_body.room_id, room.id());
            assert_eq!(ev_body.banned, false);
            assert_eq!(ev_body.banned_by, *admin.account_id());
            assert_eq!(ev_body.reason, None);

            let mut conn = context.db().acquire().await.expect("Failed conn checkout");
            let db_ban = db::room_ban::FindQuery::new(ev_body.account_id, room.id())
                .execute(&mut conn)
                .await
                .expect("Failed to query ban in db");

            assert!(db_ban.is_none());

            drop(conn);

            // Make event.create request to check that user is unbanned
            let payload = super::super::event::CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello 2" }),
                is_claim: false,
                is_persistent: true,
            };

            let messages = handle_request::<crate::app::endpoint::event::CreateHandler>(
                &mut context,
                &user,
                payload,
            )
            .await
            .expect("Event creation failed");

            assert_eq!(messages.len(), 2);

            // Assert response.
            let (_, respp, _) = find_response::<crate::db::event::Object>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
        });
    }
}
