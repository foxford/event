use anyhow::Context;
use sqlx::postgres::PgPool as Db;
use sqlx::Acquire;
use tokio::{
    sync::mpsc,
    time::{Duration as TokioDuration, MissedTickBehavior},
};
use tracing::{error, warn};
use uuid::Uuid;

use super::nats_client::NatsClient;

const DELAYED_PULL_PERIOD: TokioDuration = TokioDuration::from_secs(3);

pub struct NatsIds(Vec<Uuid>);

impl NatsIds {
    pub fn new() -> Self {
        Self(vec![])
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self(Vec::with_capacity(cap))
    }

    pub fn push(&mut self, id: Uuid) {
        self.0.push(id);
    }
}

impl IntoIterator for NatsIds {
    type Item = <Vec<Uuid> as IntoIterator>::Item;
    type IntoIter = <Vec<Uuid> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Clone)]
pub struct PullerHandle {
    notifications_tx: mpsc::UnboundedSender<Uuid>,
}

impl PullerHandle {
    pub fn notification(&self, id: Uuid) {
        if let Err(e) = self.notifications_tx.send(id) {
            error!(error = ?e, "Failed to send notification to nats thread");
        }
    }
}

pub struct Puller {
    notifications_tx: mpsc::UnboundedSender<Uuid>,
    shutdown_tx: mpsc::Sender<()>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl Puller {
    pub fn new(db: Db, nats_client: NatsClient, namespace: String) -> Self {
        let (notifications_tx, rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let join_handle =
            tokio::task::spawn(pull_loop(db, nats_client, rx, shutdown_rx, namespace));

        Self {
            join_handle,
            notifications_tx,
            shutdown_tx,
        }
    }

    pub fn handle(&self) -> PullerHandle {
        PullerHandle {
            notifications_tx: self.notifications_tx.clone(),
        }
    }

    pub async fn shutdown(self) -> Result<(), tokio::task::JoinError> {
        if let Err(e) = self.shutdown_tx.send(()).await {
            warn!(error = ?e, "Puller shutdown tx was closed");
        }
        self.join_handle.await
    }
}

async fn pull_loop(
    db: Db,
    nats_client: NatsClient,
    mut rx: mpsc::UnboundedReceiver<Uuid>,
    mut shutdown_rx: mpsc::Receiver<()>,
    namespace: String,
) {
    let mut interval = tokio::time::interval(DELAYED_PULL_PERIOD);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                let join_handle = nats_client.shutdown();
                let r = tokio::task::spawn_blocking(move || join_handle.join()).await;
                if let Err(e) = r {
                    warn!(error = ?e, "Failed to shutdown nats client properly");
                }
                return

            },
            _ = interval.tick() => {
                if let Err(e) = process_delayed_notifications(&db, &nats_client, &namespace).await {
                    error!(error = ?e, "Failed to proccess delayed notifications");
                }
            }
            Some(id) = rx.recv() => {
                if let Err(e) = process_id(&db, &nats_client, id).await {
                    error!(id = %id, error = ?e, "Failed to proccess nats notification");
                }
            }
        }
    }
}

async fn process_id(db: &Db, nats_client: &NatsClient, id: Uuid) -> anyhow::Result<()> {
    let mut conn = db
        .acquire()
        .await
        .context("Failed to acquire DB connection")?;

    let mut txn = conn
        .begin()
        .await
        .map_err(|e| anyhow!("Failed to acquire db transaction, err = {:?}", e))?;

    let notification = crate::db::notification::FindWithLockQuery::new(id)
        .execute(&mut txn)
        .await?;

    let notification = match notification {
        Some(n) => n,
        None => {
            // was already processed
            return Ok(());
        }
    };

    nats_client.publish(notification).await?;

    crate::db::notification::DeleteQuery::new(id)
        .execute(&mut txn)
        .await?;

    txn.commit()
        .await
        .map_err(|e| anyhow!("Failed to commit notification transaction, err = {:?}", e))?;

    Ok(())
}

async fn process_delayed_notifications(
    db: &Db,
    nats_client: &NatsClient,
    namespace: &str,
) -> anyhow::Result<()> {
    let mut conn = db
        .acquire()
        .await
        .context("Failed to acquire DB connection")?;

    let mut txn = conn
        .begin()
        .await
        .map_err(|e| anyhow!("Failed to acquire db transaction, err = {:?}", e))?;

    let notifications = crate::db::notification::DelayedListQuery::new(namespace)
        .execute(&mut txn)
        .await?;

    for notification in notifications {
        let id = notification.id();

        if let Err(e) = nats_client.publish(notification).await {
            if let Ok(err) = e.downcast::<std::io::Error>() {
                // Even if we erred we could have sent some notifications to nats before
                // and should commit their deletions to db
                // Exception: no responders - we should delete such notifications
                // TODO: Maybe add a counter of attempts to the notification table
                // and delete them if attempts > 10 for example?
                if err.kind() != std::io::ErrorKind::NotFound || err.to_string() != "no responders"
                {
                    break;
                }
            }
        }

        crate::db::notification::DeleteQuery::new(id)
            .execute(&mut txn)
            .await?;
    }

    txn.commit()
        .await
        .map_err(|e| anyhow!("Failed to commit notification transaction {:?}", e))?;

    Ok(())
}
