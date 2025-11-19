use super::backend::{ObjectStream, StorageBackend};
use crate::config::{ReadMode, WriteMode};
use crate::types::{error::S3Error, ObjectMetadata};
use bytes::{Bytes, BytesMut};
use futures::stream::{self, StreamExt};
use std::sync::Arc;

/// Multi-backend storage that replicates operations across multiple backends
pub struct MultiBackend {
    backends: Vec<Arc<dyn StorageBackend>>,
    primary_index: Option<usize>,
    read_mode: ReadMode,
    write_mode: WriteMode,
}

impl MultiBackend {
    /// Create a new multi-backend storage
    ///
    /// # Arguments
    /// * `backends` - List of storage backends to use (must be non-empty)
    /// * `primary_index` - Optional index of the primary backend
    /// * `read_mode` - Read consistency mode
    /// * `write_mode` - Write consistency mode
    pub fn new(
        backends: Vec<Arc<dyn StorageBackend>>,
        primary_index: Option<usize>,
        read_mode: ReadMode,
        write_mode: WriteMode,
    ) -> Self {
        tracing::info!(
            "Initializing MultiBackend with {} backends (primary_index: {:?}, read_mode: {:?}, write_mode: {:?})",
            backends.len(),
            primary_index,
            read_mode,
            write_mode
        );

        Self {
            backends,
            primary_index,
            read_mode,
            write_mode,
        }
    }

    /// Get the primary backend if specified, otherwise return None
    fn primary(&self) -> Option<&Arc<dyn StorageBackend>> {
        self.primary_index.map(|idx| &self.backends[idx])
    }

    /// Get the primary backend if specified, otherwise return the first backend
    fn primary_or_first(&self) -> &Arc<dyn StorageBackend> {
        self.primary()
            .unwrap_or_else(|| &self.backends[0])
    }

    /// Get all backends except the primary (for fallback reads)
    fn other_backends(&self) -> impl Iterator<Item = &Arc<dyn StorageBackend>> {
        let primary_idx = self.primary_index;
        self.backends
            .iter()
            .enumerate()
            .filter(move |(idx, _)| Some(*idx) != primary_idx)
            .map(|(_, backend)| backend)
    }

    /// Collect a stream into Bytes (needed for replication)
    async fn collect_stream(mut stream: ObjectStream) -> Result<Bytes, S3Error> {
        let mut data = BytesMut::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk);
        }
        Ok(data.freeze())
    }

    /// Create a new stream from Bytes
    fn bytes_to_stream(data: Bytes) -> ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }
}

#[async_trait::async_trait]
impl StorageBackend for MultiBackend {
    async fn head_bucket(&self) -> Result<(), S3Error> {
        // Query primary or first backend
        let backend = self.primary_or_first();
        tracing::debug!("HEAD bucket (using primary/first backend)");
        backend.head_bucket().await
    }

    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        // Query primary or first backend
        let backend = self.primary_or_first();
        tracing::debug!("LIST objects (using primary/first backend)");
        backend.list_objects(prefix, max_keys).await
    }

    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        match self.read_mode {
            ReadMode::BestEffort => {
                // Try primary first, then fallback to others
                if let Some(primary) = self.primary() {
                    tracing::debug!("HEAD object (trying primary backend first)");
                    match primary.head_object(key).await {
                        Ok(metadata) => return Ok(metadata),
                        Err(e) => {
                            tracing::warn!("Primary backend failed for HEAD {}: {}", key, e);
                        }
                    }
                }

                // Try other backends
                for (idx, backend) in self.other_backends().enumerate() {
                    tracing::debug!("HEAD object (trying fallback backend {})", idx);
                    match backend.head_object(key).await {
                        Ok(metadata) => return Ok(metadata),
                        Err(e) => {
                            tracing::warn!("Fallback backend {} failed for HEAD {}: {}", idx, key, e);
                        }
                    }
                }

                Err(S3Error::NoSuchKey)
            }
            ReadMode::Consistent => {
                // Only read from primary
                tracing::debug!("HEAD object (consistent mode - primary only)");
                self.primary_or_first().head_object(key).await
            }
        }
    }

    async fn get_object(&self, key: &str) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        match self.read_mode {
            ReadMode::BestEffort => {
                // Try primary first, then fallback to others
                if let Some(primary) = self.primary() {
                    tracing::debug!("GET object (trying primary backend first)");
                    match primary.get_object(key).await {
                        Ok(result) => return Ok(result),
                        Err(e) => {
                            tracing::warn!("Primary backend failed for GET {}: {}", key, e);
                        }
                    }
                }

                // Try other backends
                for (idx, backend) in self.other_backends().enumerate() {
                    tracing::debug!("GET object (trying fallback backend {})", idx);
                    match backend.get_object(key).await {
                        Ok(result) => return Ok(result),
                        Err(e) => {
                            tracing::warn!("Fallback backend {} failed for GET {}: {}", idx, key, e);
                        }
                    }
                }

                Err(S3Error::NoSuchKey)
            }
            ReadMode::Consistent => {
                // Only read from primary
                tracing::debug!("GET object (consistent mode - primary only)");
                self.primary_or_first().get_object(key).await
            }
        }
    }

    async fn put_object(&self, key: &str, body: ObjectStream) -> Result<String, S3Error> {
        // Collect the stream into memory first (needed for replication)
        tracing::debug!("PUT object: collecting stream for replication");
        let data = Self::collect_stream(body).await?;

        tracing::debug!("PUT object: collected {} bytes", data.len());

        match self.write_mode {
            WriteMode::BestEffort => {
                // Write to all backends concurrently
                tracing::info!(
                    "PUT object (best effort): writing to {} backends",
                    self.backends.len()
                );

                let tasks: Vec<_> = self
                    .backends
                    .iter()
                    .enumerate()
                    .map(|(idx, backend)| {
                        let backend = Arc::clone(backend);
                        let key = key.to_string();
                        let data = data.clone();
                        async move {
                            let stream = Self::bytes_to_stream(data);
                            let result = backend.put_object(&key, stream).await;
                            (idx, result)
                        }
                    })
                    .collect();

                let results = futures::future::join_all(tasks).await;

                // Count successes and failures
                let mut success_count = 0;
                let mut primary_etag: Option<String> = None;
                let mut any_etag: Option<String> = None;

                for (idx, result) in results {
                    match result {
                        Ok(etag) => {
                            success_count += 1;
                            if Some(idx) == self.primary_index {
                                primary_etag = Some(etag.clone());
                            }
                            if any_etag.is_none() {
                                any_etag = Some(etag);
                            }
                            tracing::info!("Backend {} successfully wrote object {}", idx, key);
                        }
                        Err(e) => {
                            tracing::error!("Backend {} failed to write object {}: {}", idx, key, e);
                        }
                    }
                }

                if success_count == 0 {
                    return Err(S3Error::InternalError(
                        "All backends failed to write object".to_string(),
                    ));
                }

                tracing::info!(
                    "PUT object (best effort): {}/{} backends succeeded",
                    success_count,
                    self.backends.len()
                );

                // Return primary etag if available, otherwise any successful etag
                Ok(primary_etag.or(any_etag).unwrap())
            }
            WriteMode::Consistent => {
                // Write to all backends concurrently, all must succeed
                tracing::info!(
                    "PUT object (consistent): writing to {} backends (all must succeed)",
                    self.backends.len()
                );

                let tasks: Vec<_> = self
                    .backends
                    .iter()
                    .enumerate()
                    .map(|(idx, backend)| {
                        let backend = Arc::clone(backend);
                        let key = key.to_string();
                        let data = data.clone();
                        async move {
                            let stream = Self::bytes_to_stream(data);
                            let result = backend.put_object(&key, stream).await;
                            (idx, result)
                        }
                    })
                    .collect();

                let results = futures::future::join_all(tasks).await;

                // Collect all results - if any failed, we fail
                let mut etags = Vec::new();
                for (idx, result) in results {
                    match result {
                        Ok(etag) => {
                            tracing::info!("Backend {} successfully wrote object {}", idx, key);
                            etags.push(etag);
                        }
                        Err(e) => {
                            tracing::error!("Backend {} failed to write object {}: {}", idx, key, e);
                            // TODO: Implement rollback - delete from successful backends
                            return Err(S3Error::InternalError(format!(
                                "Backend {} failed in consistent mode: {}",
                                idx, e
                            )));
                        }
                    }
                }

                tracing::info!("PUT object (consistent): all backends succeeded");

                // Return primary etag if available, otherwise first etag
                if let Some(primary_idx) = self.primary_index {
                    Ok(etags[primary_idx].clone())
                } else {
                    Ok(etags[0].clone())
                }
            }
        }
    }

    async fn delete_object(&self, key: &str) -> Result<(), S3Error> {
        match self.write_mode {
            WriteMode::BestEffort => {
                // Delete from all backends concurrently
                tracing::info!(
                    "DELETE object (best effort): deleting from {} backends",
                    self.backends.len()
                );

                let tasks: Vec<_> = self
                    .backends
                    .iter()
                    .enumerate()
                    .map(|(idx, backend)| {
                        let backend = Arc::clone(backend);
                        let key = key.to_string();
                        async move {
                            let result = backend.delete_object(&key).await;
                            (idx, result)
                        }
                    })
                    .collect();

                let results = futures::future::join_all(tasks).await;

                // Log failures but always succeed (S3 semantics: delete is idempotent)
                for (idx, result) in results {
                    match result {
                        Ok(_) => {
                            tracing::info!("Backend {} successfully deleted object {}", idx, key);
                        }
                        Err(e) => {
                            tracing::warn!("Backend {} failed to delete object {}: {}", idx, key, e);
                        }
                    }
                }

                Ok(())
            }
            WriteMode::Consistent => {
                // Delete from all backends concurrently, all must succeed
                tracing::info!(
                    "DELETE object (consistent): deleting from {} backends (all must succeed)",
                    self.backends.len()
                );

                let tasks: Vec<_> = self
                    .backends
                    .iter()
                    .enumerate()
                    .map(|(idx, backend)| {
                        let backend = Arc::clone(backend);
                        let key = key.to_string();
                        async move {
                            let result = backend.delete_object(&key).await;
                            (idx, result)
                        }
                    })
                    .collect();

                let results = futures::future::join_all(tasks).await;

                // All must succeed
                for (idx, result) in results {
                    match result {
                        Ok(_) => {
                            tracing::info!("Backend {} successfully deleted object {}", idx, key);
                        }
                        Err(e) => {
                            tracing::error!("Backend {} failed to delete object {}: {}", idx, key, e);
                            return Err(S3Error::InternalError(format!(
                                "Backend {} failed to delete in consistent mode: {}",
                                idx, e
                            )));
                        }
                    }
                }

                tracing::info!("DELETE object (consistent): all backends succeeded");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InMemoryStorage;
    use futures::StreamExt;

    // Helper function to convert Bytes to ObjectStream for tests
    fn bytes_to_stream(data: Bytes) -> ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }

    #[tokio::test]
    async fn test_multibackend_put_and_get_best_effort() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            Some(0), // backend1 is primary (index 0)
            ReadMode::BestEffort,
            WriteMode::BestEffort,
        );

        let key = "test-key";
        let data = Bytes::from("Hello, World!");

        // Put object
        let etag = multi
            .put_object(key, bytes_to_stream(data.clone()))
            .await
            .unwrap();
        assert!(!etag.is_empty());

        // Get object
        let (mut stream, metadata) = multi.get_object(key).await.unwrap();
        assert_eq!(metadata.key, key);
        assert_eq!(metadata.size, data.len() as u64);

        let mut collected = Vec::new();
        while let Some(result) = stream.next().await {
            collected.extend_from_slice(&result.unwrap());
        }
        assert_eq!(collected, data);

        // Verify both backends have the object
        assert!(backend1.head_object(key).await.is_ok());
        assert!(backend2.head_object(key).await.is_ok());
    }

    #[tokio::test]
    async fn test_multibackend_put_consistent_mode() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            None, // no primary
            ReadMode::Consistent,
            WriteMode::Consistent,
        );

        let key = "test-key";
        let data = Bytes::from("Consistent data");

        // Put object in consistent mode
        let etag = multi
            .put_object(key, bytes_to_stream(data.clone()))
            .await
            .unwrap();
        assert!(!etag.is_empty());

        // Verify both backends have the object
        assert!(backend1.head_object(key).await.is_ok());
        assert!(backend2.head_object(key).await.is_ok());
    }

    #[tokio::test]
    async fn test_multibackend_delete_best_effort() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            None,
            ReadMode::BestEffort,
            WriteMode::BestEffort,
        );

        let key = "test-key";
        let data = Bytes::from("data");

        // Put and then delete
        multi
            .put_object(key, bytes_to_stream(data))
            .await
            .unwrap();
        multi.delete_object(key).await.unwrap();

        // Verify both backends deleted the object
        assert!(backend1.head_object(key).await.is_err());
        assert!(backend2.head_object(key).await.is_err());
    }

    #[tokio::test]
    async fn test_multibackend_read_fallback() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            Some(0), // backend1 is primary
            ReadMode::BestEffort,
            WriteMode::BestEffort,
        );

        let key = "test-key";
        let data = Bytes::from("fallback test");

        // Put object only in backend2 (simulate primary failure)
        backend2
            .put_object(key, bytes_to_stream(data.clone()))
            .await
            .unwrap();

        // Get should succeed by falling back to backend2
        let (mut stream, _) = multi.get_object(key).await.unwrap();

        let mut collected = Vec::new();
        while let Some(result) = stream.next().await {
            collected.extend_from_slice(&result.unwrap());
        }
        assert_eq!(collected, data);
    }

    #[tokio::test]
    async fn test_multibackend_primary_selection() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            Some(1), // backend2 is primary (index 1)
            ReadMode::Consistent,
            WriteMode::Consistent,
        );

        assert_eq!(multi.primary_index, Some(1));
    }

    #[tokio::test]
    async fn test_multibackend_list_objects() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            Some(0), // backend1 is primary
            ReadMode::BestEffort,
            WriteMode::BestEffort,
        );

        // Put objects through multi-backend
        multi
            .put_object("test1", bytes_to_stream(Bytes::from("data1")))
            .await
            .unwrap();
        multi
            .put_object("test2", bytes_to_stream(Bytes::from("data2")))
            .await
            .unwrap();

        // List should return objects from primary
        let objects = multi.list_objects(None, 100).await.unwrap();
        assert_eq!(objects.len(), 2);
    }
}
