use chrono::{DateTime, Utc};
use failure::Error;
use log::info;
use serde_derive::Deserialize;
use svc_agent::mqtt::{IncomingEventProperties, IntoPublishableDump};

use crate::app::{endpoint::EventHandler, Context};

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct SayEvent {
    message: String,
}

pub(crate) struct SayHandler;

impl EventHandler for SayHandler {
    type Payload = SayEvent;

    fn handle(
        _context: &Context,
        payload: Self::Payload,
        _evp: &IncomingEventProperties,
        _start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error> {
        info!("Got dummy.say event: {}", payload.message);
        Ok(vec![])
    }
}
