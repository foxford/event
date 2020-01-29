use failure::{format_err, Error};
use http::Method;
use log::info;
use serde::{de::DeserializeOwned, ser::Serialize};
use svc_authn::{token::jws_compact, Authenticable};
use url::Url;
use uuid::Uuid;

use self::types::*;
use crate::config::Config;

pub(crate) struct Client {
    config: Config,
}

impl Client {
    pub(crate) fn new(config: Config) -> Self {
        Self { config }
    }

    /// `POST /{audience}/rooms/{room_id}`
    pub(crate) async fn create_room(
        &self,
        account: &impl Authenticable,
        audience: &str,
    ) -> Result<Room, Error> {
        self.make_request::<_, _, CreateRoomResponse>(
            account,
            Method::POST,
            audience,
            "rooms",
            &Ignore {},
        )
        .await
        .map(|payload| payload.room)
    }

    /// `POST /{audience}/rooms/{room_id}/open`
    pub(crate) async fn open_room(
        &self,
        account: &impl Authenticable,
        audience: &str,
        id: Uuid,
    ) -> Result<(), Error> {
        self.make_request::<_, _, Ignore>(
            account,
            Method::POST,
            audience,
            &format!("rooms/{}/open", id),
            &Ignore {},
        )
        .await
        .map(|_payload| ())
    }

    /// `POST /{audience}/rooms/{room_id}/close`
    pub(crate) async fn close_room(
        &self,
        account: &impl Authenticable,
        audience: &str,
        id: Uuid,
    ) -> Result<(), Error> {
        self.make_request::<_, _, Ignore>(
            account,
            Method::POST,
            audience,
            &format!("rooms/{}/close", id),
            &Ignore {},
        )
        .await
        .map(|_payload| ())
    }

    /// `PUT /{audience}/rooms/{room_id}/stream`
    pub(crate) async fn set_room_metadata(
        &self,
        account: &impl Authenticable,
        audience: &str,
        id: Uuid,
        metadata: &RoomMetadata,
    ) -> Result<(), Error> {
        self.make_request::<_, _, Ignore>(
            account,
            Method::PUT,
            audience,
            &format!("rooms/{}/stream", id),
            &metadata,
        )
        .await
        .map(|_payload| ())
    }

    /// `POST /{audience}/rooms/{room_id}/stream/transcode`
    pub(crate) async fn transcode_stream(
        &self,
        account: &impl Authenticable,
        audience: &str,
        room_id: Uuid,
    ) -> Result<(Room, Vec<Segment>), Error> {
        self.make_request::<_, _, TranscodeStreamResponse>(
            account,
            Method::POST,
            audience,
            &format!("rooms/{}/stream/transcode", room_id),
            &Ignore {},
        )
        .await
        .map(|payload| (payload.room, payload.time))
    }

    /// `GET /{audience}/rooms/{room_id}/events`
    pub(crate) async fn get_events(
        &self,
        account: &impl Authenticable,
        audience: &str,
        room_id: Uuid,
        kind: &str,
    ) -> Result<Vec<Event>, Error> {
        let mut events: Vec<Event> = vec![];
        let mut page = None;

        loop {
            let path = format!(
                "rooms/{room_id}/events?type={type}&page={page}&direction=forward",
                room_id = room_id,
                type = kind,
                page = page.unwrap_or_else(|| String::from("")),
            );

            let mut response = self
                .make_request::<_, _, GetEventsResponse>(
                    account,
                    Method::GET,
                    audience,
                    &path,
                    &Ignore {},
                )
                .await?;

            events.append(&mut response.events);

            if response.has_next_page && response.next_page.is_some() {
                page = response.next_page;
            } else {
                break;
            }
        }

        Ok(events)
    }

    /// `POST /{audience}/rooms/{room_id}/events`
    pub(crate) async fn create_event(
        &self,
        account: &impl Authenticable,
        audience: &str,
        room_id: Uuid,
        data: EventData,
    ) -> Result<Event, Error> {
        self.make_request::<_, _, CreateEventResponse>(
            account,
            Method::POST,
            audience,
            &format!("rooms/{}/events/{}", room_id, data.kind()),
            &CreateEventRequest { data },
        )
        .await
        .map(|payload| payload.event)
    }

    /// `DELETE /{audience}/rooms/{room_id}/events/{type}/{event_id}`
    pub(crate) async fn delete_event(
        &self,
        account: &impl Authenticable,
        audience: &str,
        room_id: Uuid,
        kind: &str,
        event_id: Uuid,
    ) -> Result<(), Error> {
        self.make_request::<_, _, Ignore>(
            account,
            Method::DELETE,
            audience,
            &format!("rooms/{}/events/{}/{}", room_id, kind, event_id),
            &Ignore {},
        )
        .await
        .map(|_payload| ())
    }

    ///////////////////////////////////////////////////////////////////////////

    async fn make_request<A: Authenticable, Req: Serialize, Resp: DeserializeOwned>(
        &self,
        account: &A,
        method: Method,
        audience: &str,
        path: &str,
        body: &Req,
    ) -> Result<Resp, Error> {
        let url = Url::parse(&format!(
            "{base_url}/api/v2/{audience}/{path}",
            base_url = self.config.backend_url,
            audience = audience,
            path = path,
        ))
        .map_err(|err| format_err!("Invalid backend request URL: {}", err))?;

        let token = self.token(account)?;

        info!(
            "Making backend request: {} {}/{}\n{}",
            method,
            audience,
            path,
            serde_json::to_string(body)?
        );

        // TODO: add timeout
        let mut response = surf::Request::new(method.clone(), url)
            .set_header("Authorization", &format!("Bearer {}", token))
            .body_json(&body)
            .map_err(|err| format_err!("Failed to set backend request body: {}", err))?
            .await
            .map_err(|err| format_err!("Failed to send backend request: {}", err))?;

        let status = response.status();

        if status.is_success() {
            let body = response
                .body_string()
                .await
                .map_err(|err| format_err!("Failed to read backend response: {}", err))?;

            info!("Backend response: {}", body);

            serde_json::from_str(&body)
                .map_err(|err| format_err!("Failed to parse backend response: {}", err))
        } else {
            let error = response
                .body_json::<ErrorResponse>()
                .await
                .map(|error_response| error_response.to_string())
                .unwrap_or_else(|err| format!("(also failed to parse error body: {})", err));

            Err(format_err!(
                "Backend request failed, status code = {}: {};",
                status,
                error,
            ))
        }
    }

    fn token(&self, account: &impl Authenticable) -> Result<String, Error> {
        let audience = account.as_account_id().audience();

        let (issuer, config) = self
            .config
            .authn
            .iter()
            .find(|(_, config)| config.audience().contains(audience))
            .ok_or_else(|| format_err!("Unknown audience {} in authn config", audience))?;

        jws_compact::TokenBuilder::new()
            .issuer(issuer)
            .subject(account)
            .key(config.algorithm(), config.key())
            .build()
            .map_err(|err| format_err!("Error creating backend token: {}", err))
    }
}

pub(crate) mod types;
