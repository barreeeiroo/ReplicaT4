use super::backend::{ObjectStream, StorageBackend};
use crate::types::{ObjectMetadata, error::S3Error};
use bytes::{Bytes, BytesMut};
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory storage backend for testing/development
#[derive(Clone)]
pub struct InMemoryStorage {
    objects: Arc<RwLock<HashMap<String, StoredObject>>>,
}

#[derive(Clone)]
struct StoredObject {
    data: Bytes,
    metadata: ObjectMetadata,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            objects: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn calculate_etag(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(data);
        format!("\"{}\"", hex::encode(hash))
    }
}

#[async_trait::async_trait]
impl StorageBackend for InMemoryStorage {
    // Bucket-level operations
    async fn head_bucket(&self) -> Result<(), S3Error> {
        // In-memory storage always has the bucket available
        Ok(())
    }

    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        let objects = self.objects.read().await;

        let mut results: Vec<ObjectMetadata> = objects
            .iter()
            .filter_map(|(key, obj)| {
                if let Some(prefix_str) = prefix
                    && !key.starts_with(prefix_str)
                {
                    return None;
                }

                Some(obj.metadata.clone())
            })
            .collect();

        results.sort_by(|a, b| a.key.cmp(&b.key));
        results.truncate(max_keys as usize);

        Ok(results)
    }

    // Object-level operations
    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        let objects = self.objects.read().await;

        objects
            .get(key)
            .map(|obj| obj.metadata.clone())
            .ok_or(S3Error::NoSuchKey)
    }

    async fn get_object(&self, key: &str) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        let objects = self.objects.read().await;

        let obj = objects.get(key).ok_or(S3Error::NoSuchKey)?;

        let data = obj.data.clone();
        let metadata = obj.metadata.clone();

        // Convert Bytes to a stream with a single item
        let stream = Box::pin(stream::once(async { Ok(data) }));

        Ok((stream, metadata))
    }

    async fn put_object(&self, key: &str, mut body: ObjectStream) -> Result<String, S3Error> {
        // Collect the streaming body into Bytes
        let mut data = BytesMut::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk);
        }
        let data = data.freeze();

        let etag = Self::calculate_etag(&data);

        let metadata = ObjectMetadata {
            key: key.to_string(),
            size: data.len() as u64,
            etag: etag.clone(),
            last_modified: chrono::Utc::now(),
            content_type: "binary/octet-stream".to_string(),
        };

        let stored_object = StoredObject { data, metadata };

        let mut objects = self.objects.write().await;
        objects.insert(key.to_string(), stored_object);

        Ok(etag)
    }

    async fn delete_object(&self, key: &str) -> Result<(), S3Error> {
        let mut objects = self.objects.write().await;
        objects.remove(key);

        // S3 returns success even if object doesn't exist
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    // Helper function to convert Bytes to ObjectStream for tests
    fn bytes_to_stream(data: Bytes) -> ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }

    #[tokio::test]
    async fn test_put_and_get_object() {
        let storage = InMemoryStorage::new();
        let key = "test-key";
        let data = Bytes::from("Hello, World!");

        let etag = storage
            .put_object(key, bytes_to_stream(data.clone()))
            .await
            .unwrap();
        assert!(!etag.is_empty());

        let (mut stream, metadata) = storage.get_object(key).await.unwrap();
        assert_eq!(metadata.key, key);
        assert_eq!(metadata.size, data.len() as u64);

        let mut collected = Vec::new();
        while let Some(result) = stream.next().await {
            collected.extend_from_slice(&result.unwrap());
        }
        assert_eq!(collected, data);
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let storage = InMemoryStorage::new();
        assert!(matches!(
            storage.get_object("nonexistent").await,
            Err(S3Error::NoSuchKey)
        ));
    }

    #[tokio::test]
    async fn test_delete_object() {
        let storage = InMemoryStorage::new();
        let key = "test-key";

        storage
            .put_object(key, bytes_to_stream(Bytes::from("data")))
            .await
            .unwrap();
        storage.delete_object(key).await.unwrap();

        assert!(matches!(
            storage.head_object(key).await,
            Err(S3Error::NoSuchKey)
        ));
    }

    #[tokio::test]
    async fn test_list_with_prefix() {
        let storage = InMemoryStorage::new();

        storage
            .put_object("photos/a.jpg", bytes_to_stream(Bytes::from("1")))
            .await
            .unwrap();
        storage
            .put_object("photos/b.jpg", bytes_to_stream(Bytes::from("2")))
            .await
            .unwrap();
        storage
            .put_object("docs/c.pdf", bytes_to_stream(Bytes::from("3")))
            .await
            .unwrap();

        let objects = storage.list_objects(Some("photos/"), 100).await.unwrap();
        assert_eq!(objects.len(), 2);
        assert!(objects[0].key.starts_with("photos/"));
    }

    #[tokio::test]
    async fn test_head_bucket() {
        let storage = InMemoryStorage::new();

        // In-memory storage always returns success for head_bucket
        let result = storage.head_bucket().await;
        assert!(result.is_ok());
    }
}
