use crate::storage::backend::{ObjectStream, StorageBackend};
use crate::types::{ObjectMetadata, error::S3Error};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;

pub struct S3Backend {
    client: S3Client,
    bucket: String,
    name: String,
}

/// Type alias for the receiver stream used in StreamBody
type StreamReceiver = ReceiverStream<Result<Bytes, Box<dyn std::error::Error + Send + Sync>>>;

/// Adapter to convert ObjectStream into an http_body::Body for ByteStream
/// Uses a channel-based approach to satisfy the Sync requirement while maintaining streaming
/// Only buffers up to the channel capacity (256 chunks), not the entire object
struct StreamBody {
    receiver: Arc<Mutex<StreamReceiver>>,
}

impl StreamBody {
    fn new(mut stream: ObjectStream) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(256);

        // Spawn a task to read from the non-Sync stream and forward to the channel
        tokio::spawn(async move {
            while let Some(result) = stream.next().await {
                let mapped_result = result.map_err(|e| {
                    Box::new(std::io::Error::other(format!("Stream error: {}", e)))
                        as Box<dyn std::error::Error + Send + Sync>
                });

                if tx.send(mapped_result).await.is_err() {
                    // Receiver dropped, stop reading
                    break;
                }
            }
        });

        Self {
            receiver: Arc::new(Mutex::new(ReceiverStream::new(rx))),
        }
    }
}

impl Body for StreamBody {
    type Data = Bytes;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut receiver = match self.receiver.try_lock() {
            Ok(guard) => guard,
            Err(_) => return Poll::Pending,
        };

        match Pin::new(&mut *receiver).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl S3Backend {
    /// Helper function to extract metadata from AWS SDK response
    fn extract_metadata(
        key: &str,
        content_length: Option<i64>,
        etag: Option<&str>,
        last_modified: Option<&aws_sdk_s3::primitives::DateTime>,
        content_type: Option<&str>,
    ) -> ObjectMetadata {
        let size = content_length.unwrap_or(0) as u64;
        let etag = etag.map(|s| s.to_string()).unwrap_or_default();
        let last_modified = last_modified
            .and_then(|dt| {
                let secs = dt.secs();
                chrono::DateTime::from_timestamp(secs, 0)
            })
            .unwrap_or_else(chrono::Utc::now);
        let content_type = content_type
            .map(|s| s.to_string())
            .unwrap_or_else(|| "binary/octet-stream".to_string());

        ObjectMetadata {
            key: key.to_string(),
            size,
            etag,
            last_modified,
            content_type,
        }
    }

    pub async fn new(
        name: String,
        bucket: String,
        region: String,
        endpoint: Option<String>,
        force_path_style: bool,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region));

        // Set credentials if provided
        if let (Some(key_id), Some(secret_key)) = (access_key_id, secret_access_key) {
            config_loader = config_loader.credentials_provider(
                aws_sdk_s3::config::Credentials::new(key_id, secret_key, None, None, "static"),
            );
        }

        let config = config_loader.load().await;

        let mut s3_config_builder =
            aws_sdk_s3::config::Builder::from(&config).force_path_style(force_path_style);

        // Set custom endpoint if provided
        if let Some(endpoint_url) = endpoint {
            s3_config_builder = s3_config_builder.endpoint_url(endpoint_url);
        }

        let s3_config = s3_config_builder.build();
        let client = S3Client::from_conf(s3_config);

        Ok(Self {
            client,
            bucket,
            name,
        })
    }

    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn calculate_etag(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        format!("\"{}\"", hex::encode(result))
    }
}

#[async_trait::async_trait]
impl StorageBackend for S3Backend {
    // Bucket-level operations
    async fn head_bucket(&self) -> Result<(), S3Error> {
        tracing::debug!("[{}] Checking bucket existence: {}", self.name, self.bucket);

        let result = self.client.head_bucket().bucket(&self.bucket).send().await;

        match result {
            Ok(_) => {
                tracing::debug!("[{}] Bucket exists and is accessible", self.name);
                Ok(())
            }
            Err(err) => {
                tracing::warn!(
                    "[{}] Bucket not found or not accessible: {}",
                    self.name,
                    err
                );
                Err(S3Error::NoSuchBucket)
            }
        }
    }

    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        tracing::debug!("[{}] Listing objects with prefix: {:?}", self.name, prefix);

        let mut request = self.client.list_objects_v2().bucket(&self.bucket);

        if let Some(p) = prefix {
            request = request.prefix(p);
        }

        let result = request.max_keys(max_keys).send().await;

        match result {
            Ok(output) => {
                let objects: Vec<ObjectMetadata> = output
                    .contents()
                    .iter()
                    .filter_map(|obj| {
                        let key = obj.key()?.to_string();
                        let size = obj.size().unwrap_or(0) as u64;
                        let etag = obj.e_tag().map(|s| s.to_string()).unwrap_or_default();
                        let last_modified = obj
                            .last_modified()
                            .and_then(|dt| {
                                let secs = dt.secs();
                                chrono::DateTime::from_timestamp(secs, 0)
                            })
                            .unwrap_or_else(chrono::Utc::now);

                        Some(ObjectMetadata {
                            key,
                            size,
                            etag,
                            last_modified,
                            content_type: "binary/octet-stream".to_string(),
                        })
                    })
                    .collect();

                tracing::debug!("[{}] Found {} objects", self.name, objects.len());
                Ok(objects)
            }
            Err(err) => {
                tracing::error!("[{}] Failed to list objects: {}", self.name, err);
                Err(S3Error::InternalError(format!(
                    "Failed to list objects in {}: {}",
                    self.name, err
                )))
            }
        }
    }

    // Object-level operations
    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        tracing::debug!("[{}] Getting metadata for object: {}", self.name, key);

        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match result {
            Ok(output) => {
                let metadata = Self::extract_metadata(
                    key,
                    output.content_length(),
                    output.e_tag(),
                    output.last_modified(),
                    output.content_type(),
                );
                Ok(metadata)
            }
            Err(_) => {
                tracing::warn!("[{}] Object not found: {}", self.name, key);
                Err(S3Error::NoSuchKey)
            }
        }
    }

    async fn get_object(&self, key: &str) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        tracing::debug!("[{}] Getting object: {}", self.name, key);

        let result = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match result {
            Ok(output) => {
                // Extract metadata from the response
                let metadata = Self::extract_metadata(
                    key,
                    output.content_length(),
                    output.e_tag(),
                    output.last_modified(),
                    output.content_type(),
                );

                let name = self.name.clone();
                // Convert AWS ByteStream to our generic stream
                // ByteStream wraps an SdkBody which can be converted to a stream of frames
                let body = output.body.into_inner();
                let stream = body.into_data_stream().map(move |result| {
                    result.map_err(|e| {
                        tracing::error!("[{}] Failed to read object chunk: {}", name, e);
                        S3Error::InternalError(format!("Failed to read object: {}", e))
                    })
                });

                Ok((Box::pin(stream), metadata))
            }
            Err(_err) => {
                tracing::warn!("[{}] Object not found: {}", self.name, key);
                Err(S3Error::NoSuchKey)
            }
        }
    }

    async fn put_object(&self, key: &str, body: ObjectStream) -> Result<String, S3Error> {
        tracing::debug!("[{}] Putting object (streaming): {}", self.name, key);

        // Wrap the stream in our Body adapter for true streaming
        // Only buffers up to 256 chunks in the channel, not the entire object
        let stream_body = StreamBody::new(body);

        // Convert to ByteStream using the Body adapter
        let body_stream = ByteStream::from_body_1_x(stream_body);

        let result = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body_stream)
            .send()
            .await;

        match result {
            Ok(output) => {
                tracing::info!("[{}] Successfully stored object: {}", self.name, key);
                // Get ETag from S3 response
                let etag = output
                    .e_tag()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| Self::calculate_etag(&[]));
                Ok(etag)
            }
            Err(err) => {
                tracing::error!("[{}] Failed to put object: {}", self.name, err);
                Err(S3Error::InternalError(format!(
                    "Failed to store object in {}: {}",
                    self.name, err
                )))
            }
        }
    }

    async fn delete_object(&self, key: &str) -> Result<(), S3Error> {
        tracing::debug!("[{}] Deleting object: {}", self.name, key);

        let result = self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match result {
            Ok(_) => {
                tracing::info!("[{}] Successfully deleted object: {}", self.name, key);
                Ok(())
            }
            Err(err) => {
                tracing::error!("[{}] Failed to delete object: {}", self.name, err);
                // S3 delete is idempotent - returns success even if object doesn't exist
                Ok(())
            }
        }
    }
}
