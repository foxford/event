use failure::{format_err, Error};
use http::Method;
use serde::{de::DeserializeOwned, ser::Serialize};
use svc_authn::{token::jws_compact, Authenticable};
use url::Url;

use self::types::*;
use crate::config::Config;

pub(crate) struct Client {
    config: Config,
}

impl Client {
    pub(crate) fn new(config: Config) -> Self {
        Self { config }
    }

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
            &CreateRoomRequest {},
        )
        .await
        .map(|payload| payload.room)
    }

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
        println!("TOKEN: {}", token);

        // TODO: add timeout
        let mut response = surf::Request::new(method, url)
            .set_header("Authorization", &format!("Bearer {}", token))
            .body_json(&body)
            .map_err(|err| format_err!("Failed to set backend request body: {}", err))?
            .await
            .map_err(|err| format_err!("Failed to send room creation request: {}", err))?;

        let status = response.status();

        if status.is_success() {
            response
                .body_json::<Resp>()
                .await
                .map_err(|err| format_err!("Failed to parse room creation response: {}", err))
        } else {
            let error = response
                .body_json::<ErrorResponse>()
                .await
                .map(|error_response| error_response.to_string())
                .unwrap_or_else(|err| format!("(also failed to parse error body: {})", err));

            Err(format_err!(
                "Backend request failed, status code = {}: {}",
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
