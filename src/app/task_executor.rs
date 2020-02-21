use std::future::Future;
use std::sync::Arc;

use futures::{executor::ThreadPool, task::SpawnExt};
use log::{error, warn};
use svc_agent::mqtt::{Agent, IntoPublishableDump, ResponseStatus};
use svc_error::{extension::sentry, Error as SvcError};

use crate::app::message_handler::publish_message;

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct TaskExecutor {
    agent: Agent,
    thread_pool: Arc<ThreadPool>,
}

impl TaskExecutor {
    pub(crate) fn new(agent: Agent, thread_pool: Arc<ThreadPool>) -> Self {
        Self { agent, thread_pool }
    }

    pub(crate) fn run(
        &self,
        task: impl Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'static,
    ) -> Result<(), SvcError> {
        let mut agent = self.agent.clone();

        self.thread_pool
            .spawn(async move {
                for message in task.await {
                    if let Err(err) = publish_message(&mut agent, message) {
                        let svc_error = SvcError::builder()
                            .status(ResponseStatus::INTERNAL_SERVER_ERROR)
                            .kind("task_executor", "Failed to publish message")
                            .detail(&err.to_string())
                            .build();

                        error!("{}", svc_error);

                        sentry::send(svc_error).unwrap_or_else(|err| {
                            warn!("Error sending error to Sentry: {}", err);
                        });

                        break;
                    }
                }
            })
            .map_err(|err| {
                SvcError::builder()
                    .status(ResponseStatus::INTERNAL_SERVER_ERROR)
                    .kind("task_executor", "Failed to spawn asynchronous task")
                    .detail(&err.to_string())
                    .build()
            })
    }
}
