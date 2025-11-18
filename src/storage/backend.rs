use crate::types::{ObjectMetadata, error::S3Error};
use bytes::Bytes;

/// Storage backend trait - implement this for different storage backends
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    async fn get_object(&self, key: &str) -> Result<Bytes, S3Error>;
    async fn put_object(&self, key: &str, data: Bytes) -> Result<String, S3Error>;
    async fn delete_object(&self, key: &str) -> Result<(), S3Error>;
    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error>;
    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error>;
}
