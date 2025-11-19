use crate::types::{ObjectMetadata, error::S3Error};
use bytes::Bytes;
use futures::stream::Stream;
use std::pin::Pin;

/// Type alias for object data stream (used for both input and output)
pub type ObjectStream = Pin<Box<dyn Stream<Item = Result<Bytes, S3Error>> + Send>>;

/// Storage backend trait - implement this for different storage backends
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    // Bucket-level operations

    /// Check if bucket exists and user has access to it
    /// Returns Ok(()) if bucket exists and is accessible, Err(S3Error::NoSuchBucket) otherwise
    async fn head_bucket(&self) -> Result<(), S3Error>;

    /// List objects in the bucket with optional prefix filtering
    /// Returns a vector of object metadata, limited by max_keys
    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error>;

    // Object-level operations

    /// Get object metadata without retrieving the object body
    /// Returns object metadata if found, Err(S3Error::NoSuchKey) otherwise
    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error>;

    /// Get an object as a stream of bytes along with its metadata
    /// Returns a tuple of (stream, metadata) if found, Err(S3Error::NoSuchKey) otherwise
    async fn get_object(&self, key: &str) -> Result<(ObjectStream, ObjectMetadata), S3Error>;

    /// Store an object from a streaming body
    /// Returns the ETag of the stored object
    async fn put_object(&self, key: &str, body: ObjectStream) -> Result<String, S3Error>;

    /// Delete an object from storage
    /// Returns Ok(()) regardless of whether the object existed (idempotent)
    async fn delete_object(&self, key: &str) -> Result<(), S3Error>;
}
