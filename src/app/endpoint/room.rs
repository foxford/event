use async_trait::async_trait;
use chrono::{DateTime, Utc};
use failure::Error;
use serde_derive::{Deserialize, Serialize};
use svc_agent::mqtt::{
    IncomingRequestProperties, IntoPublishableDump, OutgoingResponse, ResponseStatus,
    ShortTermTimingProperties,
};

use crate::app::{endpoint::RequestHandler, Context, API_VERSION};
use crate::backend::types::Room;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    audience: String,
    description: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CreateResponse {
    room: Room,
}

pub(crate) struct CreateHandler;

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;
    const ERROR_TITLE: &'static str = "Failed to create room";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error> {
        let room = context
            .backend()
            .create_room(&payload.audience, &payload.description)
            .await?;

        let timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = reqp.to_response(ResponseStatus::CREATED, timing);
        let resp = OutgoingResponse::unicast(CreateResponse { room }, props, reqp, API_VERSION);
        Ok(vec![Box::new(resp) as Box<dyn IntoPublishableDump>])
    }
}
