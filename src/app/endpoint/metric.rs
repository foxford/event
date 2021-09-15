use async_std::stream;
use async_trait::async_trait;
use serde_derive::Deserialize;
use svc_agent::mqtt::IncomingEventProperties;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;

#[derive(Debug, Deserialize)]
pub(crate) struct PullPayload {
    #[serde(default = "default_duration")]
    duration: u64,
}

fn default_duration() -> u64 {
    5
}

pub(crate) struct PullHandler;

#[async_trait]
impl EventHandler for PullHandler {
    type Payload = PullPayload;

    async fn handle<C: Context>(
        _context: &mut C,
        _payload: Self::Payload,
        _evp: &IncomingEventProperties,
    ) -> Result {
        Ok(Box::new(stream::empty()))
    }
}
