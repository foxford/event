use std::future::Future;

use async_std::task;
use log::{error, warn};
use svc_agent::mqtt::{Agent, IntoPublishableDump, ResponseStatus};
use svc_error::{extension::sentry, Error as SvcError};

use crate::app::message_handler::publish_message;

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct AppTaskExecutor {
    agent: Agent,
}

impl AppTaskExecutor {
    pub(crate) fn new(agent: Agent) -> Self {
        Self { agent }
    }
}

pub(crate) trait TaskExecutor {
    fn run(
        &self,
        task: impl Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'static,
    );
}

impl TaskExecutor for AppTaskExecutor {
    fn run(
        &self,
        task: impl Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'static,
    ) {
        let mut agent = self.agent.clone();

        task::spawn(async move {
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
        });
    }
}
