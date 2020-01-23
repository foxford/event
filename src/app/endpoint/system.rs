use chrono::{DateTime, Utc};
use failure::Error;
use serde_derive::{Deserialize, Serialize};
use svc_agent::mqtt::{
    IncomingRequestProperties, IntoPublishableDump, OutgoingResponse, ResponseStatus,
    ShortTermTimingProperties,
};

use crate::app::{endpoint::RequestHandler, Context, API_VERSION};

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct PingRequest {
    message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PingResponse {
    message: String,
}

pub(crate) struct PingHandler;

impl RequestHandler for PingHandler {
    type Payload = PingRequest;
    const ERROR_TITLE: &'static str = "Failed to ping";

    fn handle(
        _context: &Context,
        _payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error> {
        let resp_payload = PingResponse {
            message: String::from("pong"),
        };

        let timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = reqp.to_response(ResponseStatus::OK, timing);
        let resp = OutgoingResponse::unicast(resp_payload, props, reqp, API_VERSION);
        Ok(vec![Box::new(resp) as Box<dyn IntoPublishableDump>])
    }
}
