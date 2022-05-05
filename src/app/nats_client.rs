use std::thread;

use anyhow::Context;
use crossbeam_channel::{unbounded, Receiver, Sender};
use nats::{jetstream::JetStream, Message};
use tokio::sync::oneshot;
use tracing::{error, info, warn};

use crate::db::notification::Object as Notification;

#[derive(Debug)]
enum Cmd {
    Publish {
        msg: Notification,
        resp_chan: oneshot::Sender<std::io::Result<()>>,
    },
    Shutdown,
}

fn nats_loop(js: JetStream, rx: Receiver<Cmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Publish { msg, resp_chan } => {
                let payload = serde_json::to_string(&msg).unwrap();
                let nats_msg = Message::new(msg.topic(), None, &payload, None);

                let resp = js.publish_message(&nats_msg).map(|ack| {
                    info!(topic = %msg.topic(), seq = ack.sequence, stream = %ack.stream, "Published message through nats");
                }).map_err(|err|
                {
                    error!(
                        topic = %msg.topic(),
                        error = ?err,
                        "Failed to publish message through nats"
                    );
                    err
                });

                if resp_chan.send(resp).is_err() {
                    warn!("NatsClient failed to notify publisher through oneshot channel");
                }
            }
            Cmd::Shutdown => break,
        }
    }
}

#[derive(Debug)]
pub struct NatsClient {
    tx: Sender<Cmd>,
    join_handle: thread::JoinHandle<()>,
}

impl NatsClient {
    pub fn new(nats_url: &str) -> anyhow::Result<Self> {
        let connection = nats::Options::with_credentials("nats.creds").connect(nats_url)?;

        let jetstream = nats::jetstream::new(connection);

        let (tx, rx) = unbounded();

        let join_handle = thread::spawn(move || nats_loop(jetstream, rx));
        Ok(Self { tx, join_handle })
    }

    pub async fn publish(&self, msg: Notification) -> Result<(), anyhow::Error> {
        let (resp_chan, resp_rx) = oneshot::channel();
        self.tx
            .try_send(Cmd::Publish { msg, resp_chan })
            .context("Failed to send cmd to nats loop")?;
        resp_rx
            .await
            .context("Failed to await response from nats loop")?
            .context("Failed to send message to nats")
    }

    pub fn shutdown(self) -> thread::JoinHandle<()> {
        let _ = self.tx.try_send(Cmd::Shutdown);

        self.join_handle
    }
}
