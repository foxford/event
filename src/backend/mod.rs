// TOKEN=$(jwt encode -A HS256 -a dev.usr.example.org -i iam.dev.usr.example.org -S secret -p "")
//
// curl http://0.0.0.0:8000/api/v2/dev.usr.example.org/rooms \
//   -H "Authorization: Bearer $TOKEN" \
//   -H "Content-Type: application/json" \
//   -d '{"description": "hello"}'

use failure::{format_err, Error};
use reqwest::Client as HttpClient;
use serde::de::DeserializeOwned;
use serde_derive::Deserialize;
use svc_agent::{AccountId, AgentId};
use svc_authn::{token::jws_compact, Authenticable};

use self::types::*;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Config {
    base_url: String,
    #[serde(flatten)]
    jwt: crate::config::JwtConfig,
}

pub(crate) struct Client {
    config: Config,
    me: AgentId,
    http_client: HttpClient,
}

impl Client {
    pub(crate) fn new(config: Config, me: AgentId) -> Self {
        Self {
            config,
            me,
            http_client: HttpClient::new(),
        }
    }

    pub(crate) async fn create_room(
        &self,
        audience: &str,
        description: &str,
    ) -> Result<Room, Error> {
        let url = format!(
            "{base_url}/{audience}/rooms",
            base_url = self.config.base_url,
            audience = audience
        );

        let response = self
            .http_client
            .post(&url)
            .json(&CreateRoomRequest { description })
            .bearer_auth(self.token(audience)?)
            .send()
            .await
            .map_err(|err| format_err!("Failed to send room creation request: {}", err))?;

        Self::handle_response::<CreateRoomResponse>(response)
            .await
            .map(|payload| payload.room)
    }

    fn token(&self, audience: &str) -> Result<String, Error> {
        jws_compact::TokenBuilder::new()
            .issuer(&self.me.as_account_id().audience().to_string())
            .subject(&AccountId::new("event-service", audience))
            .key(self.config.jwt.algorithm, self.config.jwt.key.as_slice())
            .build()
            .map_err(|err| format_err!("Error creating backend token: {}", err))
    }

    async fn handle_response<R: DeserializeOwned>(response: reqwest::Response) -> Result<R, Error> {
        let status = response.status();

        if status.is_success() {
            response
                .json::<R>()
                .await
                .map_err(|err| format_err!("Failed to parse room creation response: {}", err))
        } else {
            let error = response
                .json::<ErrorResponse>()
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
}

pub(crate) mod types;
