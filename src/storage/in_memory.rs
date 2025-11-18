use super::backend::StorageBackend;
use crate::types::{error::S3Error, ObjectMetadata};
use bytes::Bytes;
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
    async fn get_object(&self, key: &str) -> Result<Bytes, S3Error> {
        let objects = self.objects.read().await;

        objects
            .get(key)
            .map(|obj| obj.data.clone())
            .ok_or(S3Error::NoSuchKey)
    }

    async fn put_object(&self, key: &str, data: Bytes) -> Result<String, S3Error> {
        let etag = Self::calculate_etag(&data);

        let metadata = ObjectMetadata {
            key: key.to_string(),
            size: data.len() as u64,
            etag: etag.clone(),
            last_modified: chrono::Utc::now(),
            content_type: "binary/octet-stream".to_string(),
        };

        let stored_object = StoredObject {
            data,
            metadata,
        };

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

    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        let objects = self.objects.read().await;

        objects
            .get(key)
            .map(|obj| obj.metadata.clone())
            .ok_or(S3Error::NoSuchKey)
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
                    && !key.starts_with(prefix_str) {
                        return None;
                    }

                Some(obj.metadata.clone())
            })
            .collect();

        results.sort_by(|a, b| a.key.cmp(&b.key));
        results.truncate(max_keys as usize);

        Ok(results)
    }
}
