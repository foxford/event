use serde::de::DeserializeOwned;
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;

#[derive(Debug, Deserialize)]
pub struct OutgoingEnvelope {
    payload: String,
    properties: OutgoingEnvelopeProperties,
    #[serde(skip)]
    topic: String,
}

impl OutgoingEnvelope {
    pub fn payload<P: DeserializeOwned>(&self) -> P {
        serde_json::from_str::<P>(&self.payload).expect("Failed to parse payload")
    }

    pub fn properties(&self) -> &OutgoingEnvelopeProperties {
        &self.properties
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub(super) fn set_topic(&mut self, topic: &str) -> &mut Self {
        self.topic = topic.to_owned();
        self
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum OutgoingEnvelopeProperties {
    Event(OutgoingEventProperties),
    Response(OutgoingResponseProperties),
    Request(OutgoingRequestProperties),
}

#[derive(Debug, Deserialize)]
pub struct OutgoingEventProperties {
    label: String,
}

impl OutgoingEventProperties {
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Debug, Deserialize)]
pub struct OutgoingResponseProperties {
    status: String,
    #[allow(dead_code)]
    correlation_data: String,
}

impl OutgoingResponseProperties {
    pub fn status(&self) -> ResponseStatus {
        ResponseStatus::from_bytes(self.status.as_bytes()).expect("Invalid status code")
    }
}

#[derive(Debug, Deserialize)]
pub struct OutgoingRequestProperties {
    #[allow(dead_code)]
    method: String,
}
