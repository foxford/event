use std::sync::Arc;

use chrono::Duration;
use svc_agent::Authenticable;
use svc_authz::{ClientMap, Error, IntentObject};

use crate::metrics::Metrics;

#[derive(Clone)]
pub(crate) struct Authz {
    metrics: Arc<Metrics>,
    client_map: Arc<ClientMap>,
}

impl Authz {
    pub fn new(client_map: ClientMap, metrics: Arc<Metrics>) -> Self {
        Self {
            metrics,
            client_map: Arc::new(client_map),
        }
    }

    pub async fn authorize<A>(
        &self,
        audience: String,
        subject: A,
        object: Box<dyn IntentObject>,
        action: String,
    ) -> Result<Duration, Error>
    where
        A: Authenticable,
    {
        let _timer = self.metrics.authorization_time.start_timer();
        self.client_map
            .authorize(audience, subject, object, action)
            .await
    }

    pub async fn ban<A>(
        &self,
        audience: String,
        subject: A,
        object: Box<dyn IntentObject>,
        value: bool,
        seconds: usize,
    ) -> Result<(), Error>
    where
        A: Authenticable,
    {
        self.client_map
            .ban(audience, subject, object, value, seconds)
            .await
    }
}
