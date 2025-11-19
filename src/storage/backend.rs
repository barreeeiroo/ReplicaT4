use crate::types::{ObjectMetadata, error::S3Error};
use bytes::Bytes;
use futures::stream::Stream;
use std::pin::Pin;

/// Type alias for object data stream
pub type ObjectStream = Pin<Box<dyn Stream<Item = Result<Bytes, S3Error>> + Send>>;

/// Storage backend trait - implement this for different storage backends
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Get an object as a stream of bytes along with its metadata
    async fn get_object(&self, key: &str) -> Result<(ObjectStream, ObjectMetadata), S3Error>;

    async fn put_object(&self, key: &str, data: Bytes) -> Result<String, S3Error>;
    async fn delete_object(&self, key: &str) -> Result<(), S3Error>;
    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error>;
    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error>;

    /// Check if bucket exists and user has access to it
    async fn head_bucket(&self) -> Result<(), S3Error>;
}
