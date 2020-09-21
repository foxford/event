use std::sync::Arc;

use async_std::stream::StreamExt;
use async_std::sync::Sender;
use chrono::{serde::ts_seconds, DateTime, Utc};
use log::{error, warn};
use serde_derive::Deserialize;
use svc_authn::AccountId;

use crate::app::Context;
use crate::app::MessageHandler;

#[derive(Clone)]
pub(crate) struct StatsRoute<C: Context> {
    message_handler: Arc<MessageHandler<C>>,
    agent_label: String,
    id: AccountId,
}

#[derive(Debug, Clone)]
struct StatsHandle {
    tx: async_std::sync::Sender<StatsRouteCommand>,
}

enum StatsRouteCommand {
    GetStats(Sender<Result<String, String>>),
}

impl<C: Context + Send + 'static> StatsRoute<C> {
    pub fn start(config: crate::app::config::Config, message_handler: Arc<MessageHandler<C>>) {
        if let Some(metrics_conf) = config.metrics {
            let (tx, mut rx) = async_std::sync::channel(1000);
            let handle = StatsHandle { tx };

            let route = Self {
                message_handler,
                id: config.id.clone(),
                agent_label: config.agent_label,
            };

            async_std::task::spawn(async move {
                loop {
                    if let Some(x) = rx.next().await {
                        match x {
                            StatsRouteCommand::GetStats(chan) => {
                                chan.send(route.get_stats()).await;
                            }
                        }
                    }
                }
            });

            std::thread::Builder::new()
                .name(String::from("tide-metrics-server"))
                .spawn(move || {
                    warn!(
                        "StatsRoute listening on http://{}",
                        metrics_conf.http.bind_address
                    );

                    let mut app = tide::with_state(handle);
                    app.at("/metrics")
                        .get(|req: tide::Request<StatsHandle>| async move {
                            match req.state().get_stats().await {
                                Ok(Ok(text)) => {
                                    let mut res = tide::Response::new(200);
                                    res.set_body(tide::Body::from_string(text));
                                    Ok(res)
                                }
                                Ok(Err(e)) => {
                                    error!("Something went wrong: {:?}", e);
                                    let mut res = tide::Response::new(500);
                                    res.set_body(tide::Body::from_string(
                                        "Something went wrong".into(),
                                    ));
                                    Ok(res)
                                }
                                Err(e) => {
                                    error!("Something went wrong: {:?}", e);
                                    let mut res = tide::Response::new(500);
                                    res.set_body(tide::Body::from_string(
                                        "Something went wrong".into(),
                                    ));
                                    Ok(res)
                                }
                            }
                        });

                    if let Err(e) =
                        async_std::task::block_on(app.listen(metrics_conf.http.bind_address))
                    {
                        error!("Tide future completed with error = {:?}", e);
                    }
                })
                .expect("Failed to spawn tide-metrics-server thread");
        }
    }

    fn get_stats(&self) -> Result<String, String> {
        let mut acc = String::from("");

        let metrics = self.message_handler.context().get_metrics(5)?;

        for metric in metrics {
            let metric = serde_json::to_string(&metric)
                .and_then(|json| serde_json::from_str::<MetricHelper>(&json));
            match metric {
                Ok(metric) => {
                    acc.push_str(&format!(
                        "{}{{agent_label=\"{}\",account_id=\"{}\"}} {}\n",
                        metric.key, self.agent_label, self.id, metric.value
                    ));
                }
                Err(e) => warn!(
                    "Conversion from Metric to MetricHelper failed, reason = {:?}",
                    e
                ),
            }
        }
        Ok(acc)
    }
}

#[derive(Deserialize)]
struct MetricHelper {
    pub value: serde_json::Value,
    #[serde(rename = "metric")]
    pub key: String,
    #[serde(with = "ts_seconds")]
    pub timestamp: DateTime<Utc>,
}

impl StatsHandle {
    pub async fn get_stats(&self) -> Result<Result<String, String>, async_std::sync::RecvError> {
        let (tx, rx) = async_std::sync::channel(1);
        self.tx.send(StatsRouteCommand::GetStats(tx)).await;

        rx.recv().await
    }
}
