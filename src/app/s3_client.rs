use std::env::var;

use anyhow::Result as AnyResult;
use futures::channel::mpsc::channel as mpsc_channel;
use futures::channel::mpsc::Sender;
use futures::channel::oneshot::channel as once_channel;
use futures::channel::oneshot::Sender as OnceSender;
use futures::StreamExt;

use rusoto_core::Region;
use rusoto_credential::StaticProvider;
use rusoto_s3::S3Client as RusotoClient;
use rusoto_s3::{PutObjectOutput, PutObjectRequest, S3};

type Message = (PutObjectRequest, OnceSender<AnyResult<PutObjectOutput>>);

#[derive(Debug, Clone)]
pub struct S3Client {
    sender: Sender<Message>,
}

impl S3Client {
    pub fn new() -> Option<Self> {
        Self::new_with_client(build_client()?)
    }

    pub fn new_with_client(s3_client: RusotoClient) -> Option<Self> {
        let (sender, mut receiver) = mpsc_channel::<Message>(10);

        // TODO: on shutdown await all s3 client tasks to finish
        tokio::task::spawn(async move {
            while let Some((request, response_sender)) = receiver.next().await {
                let s3_client = s3_client.clone();
                tokio::spawn(async move {
                    let response = s3_client
                        .put_object(request)
                        .await
                        .map_err(|e| anyhow!("Failed to upload events to s3, reason = {:?}", e));

                    if let Err(e) = response_sender.send(response) {
                        error!(
                            crate::LOG,
                            "Failed to send S3 response to requesting thread, reason = {:?}", e
                        );
                    }
                });
            }
        });

        Some(Self { sender })
    }

    pub async fn put_object(&self, request: PutObjectRequest) -> AnyResult<PutObjectOutput> {
        let (tx, rx) = once_channel();
        self.sender
            .clone()
            .try_send((request, tx))
            .map_err(|_| anyhow!("Put object send error"))?;
        rx.await?
    }
}

fn build_client() -> Option<RusotoClient> {
    let (key, secret, endpoint, region) = match get_aws_creds() {
        Some(creds) => creds,
        None => {
            warn!(
                crate::LOG,
                "No S3 credentials specified, room.dump_events will err"
            );
            return None;
        }
    };

    let region = Region::Custom {
        name: region,
        endpoint,
    };

    let credentials = StaticProvider::new_minimal(key, secret);
    let client = rusoto_s3::S3Client::new_with(
        rusoto_core::request::HttpClient::new().expect("Failed to build rusoto http client"),
        credentials,
        region,
    );

    Some(client)
}

fn get_aws_creds() -> Option<(String, String, String, String)> {
    let key = var("AWS_ACCESS_KEY_ID").ok()?;
    let secret = var("AWS_SECRET_ACCESS_KEY").ok()?;
    let endpoint = var("AWS_ENDPOINT").ok()?;
    let region = var("AWS_REGION").ok()?;
    Some((key, secret, endpoint, region))
}
