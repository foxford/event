use std::sync::Arc;

use axum::{
    async_trait,
    extract::{Extension, FromRequestParts, Json},
    http::{request::Parts, StatusCode},
};
use svc_agent::{AccountId, AgentId};
use svc_authn::jose::ConfigMap as AuthnConfig;
use svc_authn::token::jws_compact::extract::decode_jws_compact_with_config;
//use svc_error::Error;
use serde::{Deserialize, Serialize};
use std::{error, fmt};
use tracing::{field, Span};

/// Extracts `AccountId` from "Authorization: Bearer ..." headers.
pub struct AccountIdExtractor(pub AccountId);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AccountIdExtractor {
    type Rejection = (StatusCode, Json<Error>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        use axum::RequestPartsExt;
        let Extension(authn) = parts
            .extract::<Extension<Arc<AuthnConfig>>>()
            .await
            .ok()
            .ok_or((
                StatusCode::UNAUTHORIZED,
                Json(Error::new(
                    "no_authn_config",
                    "No authn config",
                    StatusCode::UNAUTHORIZED,
                )),
            ))?;

        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|x| x.to_str().ok())
            .and_then(|x| x.get("Bearer ".len()..))
            .ok_or((
                StatusCode::UNAUTHORIZED,
                Json(Error::new(
                    "invalid_authentication",
                    "no/invalid Authorization header",
                    StatusCode::UNAUTHORIZED,
                )),
            ))?;

        let claims = decode_jws_compact_with_config::<String>(auth_header, &authn)
            .map_err(|e| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(Error::new(
                        "invalid_authentication:decode_jws_compact_with_config",
                        e.to_string(),
                        StatusCode::UNAUTHORIZED,
                    )),
                )
            })?
            .claims;
        let account_id = AccountId::new(claims.subject(), claims.audience());

        Span::current().record("account_id", &field::display(&account_id));

        Ok(Self(account_id))
    }
}

/// Extracts `AgentId`. User should provide 2 headers to make this work:
///
/// * "Authorization: Bearer <token>"
/// * "X-Agent-Label: <label>"
pub struct AgentIdExtractor(pub AgentId);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AgentIdExtractor {
    type Rejection = (StatusCode, Json<Error>);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let agent_label = parts
            .headers
            .get("X-Agent-Label")
            .and_then(|x| x.to_str().ok())
            .unwrap_or("http")
            .to_string();

        let AccountIdExtractor(account_id) =
            AccountIdExtractor::from_request_parts(parts, state).await?;

        // TODO: later missing header will be hard error
        // .ok_or((
        //     StatusCode::UNAUTHORIZED,
        //     Json(Error::new(
        //         "invalid_agent_label",
        //         "Invalid agent label",
        //         StatusCode::UNAUTHORIZED,
        //     )),
        // ))?;

        let agent_id = AgentId::new(agent_label, account_id);

        Span::current().record("agent_id", &field::display(&agent_id));

        Ok(Self(agent_id))
    }
}

/// Error object.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Error {
    #[serde(rename = "type")]
    kind: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl Error {
    /// Create an error object.
    pub fn new<S: Into<String>, K: Into<String>>(kind: S, title: K, _status: StatusCode) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            detail: None,
        }
    }
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "[{}] {}", self.kind, self.title)?;

        if let Some(ref detail) = self.detail {
            write!(fmt, ": {detail}")?;
        }

        Ok(())
    }
}

impl From<StatusCode> for Error {
    fn from(status: StatusCode) -> Self {
        let title = status.canonical_reason().unwrap_or("Unknown status code");
        Self {
            kind: String::from("about:blank"),
            title: title.to_owned(),
            detail: None,
        }
    }
}
