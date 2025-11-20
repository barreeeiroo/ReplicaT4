use super::MultiBackend;
use crate::config::WriteMode;
use crate::storage::backend::{ObjectStream, StorageBackend};
use crate::types::error::S3Error;
use bytes::Bytes;
use futures::stream::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

impl MultiBackend {
    pub(super) async fn put_object_impl(
        &self,
        key: &str,
        body: ObjectStream,
    ) -> Result<String, S3Error> {
        match self.write_mode {
            WriteMode::AsyncReplication => {
                // Stream to primary while buffering chunks for background replication
                tracing::debug!(
                    "PUT object (async): streaming to primary with background replication"
                );
                self.put_object_async_replication_streaming(key, body).await
            }
            WriteMode::MultiSync => {
                // Stream chunks to all backends without full buffering
                tracing::debug!("PUT object (multi-sync): streaming to all backends");
                self.put_object_multi_sync_streaming(key, body).await
            }
        }
    }

    /// Stream to primary, then replicate to other backends in background via GET streaming
    async fn put_object_async_replication_streaming(
        &self,
        key: &str,
        body: ObjectStream,
    ) -> Result<String, S3Error> {
        let primary = self.primary();
        tracing::info!("PUT object (async replication streaming): streaming to primary");

        // Upload to primary backend (true streaming, no buffering)
        let etag = primary.put_object(key, body).await?;

        tracing::info!("Primary backend successfully wrote object {}", key);

        // Spawn background tasks to replicate to other backends
        // These will GET from primary and stream to other backends
        self.spawn_background_replication_tasks_streaming(key);

        Ok(etag)
    }

    /// Legacy method using full buffering (kept for reference, unused)
    #[allow(dead_code)]
    async fn put_object_async_replication(
        &self,
        key: &str,
        data: Bytes,
    ) -> Result<String, S3Error> {
        // Write to primary backend immediately
        let primary = self.primary();
        tracing::info!("PUT object (async replication): writing to primary backend immediately");

        let stream = Self::bytes_to_stream(data.clone());
        let etag = primary.put_object(key, stream).await?;

        tracing::info!("Primary backend successfully wrote object {}", key);

        // Spawn background tasks to replicate to other backends
        self.spawn_background_replication_tasks(key, data);

        Ok(etag)
    }

    /// Stream a single source to multiple backends concurrently
    /// Returns (backend_index, result) for each backend
    async fn broadcast_stream_to_backends(
        backends: Vec<(usize, Arc<dyn StorageBackend>)>,
        key: &str,
        mut stream: ObjectStream,
    ) -> Result<Vec<(usize, Result<String, S3Error>)>, S3Error> {
        let num_backends = backends.len();

        // Create a channel for each backend
        let mut senders = Vec::with_capacity(num_backends);
        let mut backend_tasks = Vec::with_capacity(num_backends);

        for (idx, backend) in backends {
            // Create channel with a buffer size of 256 chunks
            // This provides breathing room for backends with varying upload speeds
            let (tx, rx) = mpsc::channel::<Result<Bytes, S3Error>>(256);
            senders.push(tx);

            // Spawn a task for each backend to consume from its channel
            let key = key.to_string();
            let task = tokio::spawn(async move {
                let stream: ObjectStream = Box::pin(ReceiverStream::new(rx));
                let result = backend.put_object(&key, stream).await;
                (idx, result)
            });
            backend_tasks.push(task);
        }

        // Read chunks from the incoming stream and broadcast to all backends
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    // Send the chunk to all backends
                    // Note: Bytes::clone() is cheap (Arc-based)
                    for sender in &senders {
                        if sender.send(Ok(chunk.clone())).await.is_err() {
                            tracing::warn!("Channel closed while sending chunk");
                        }
                    }
                }
                Err(e) => {
                    // Error reading from source stream, propagate to all backends
                    for sender in &senders {
                        let _ = sender.send(Err(e.clone())).await;
                    }
                    return Err(e);
                }
            }
        }

        // Drop senders to close channels (signal EOF to backend tasks)
        drop(senders);

        // Wait for all backend tasks and collect results
        let mut results = Vec::with_capacity(num_backends);
        for task in backend_tasks {
            let task_result = task
                .await
                .map_err(|e| S3Error::InternalError(format!("Backend task panicked: {}", e)))?;
            results.push(task_result);
        }

        Ok(results)
    }

    /// Stream chunks to all backends simultaneously without buffering the entire object
    async fn put_object_multi_sync_streaming(
        &self,
        key: &str,
        body: ObjectStream,
    ) -> Result<String, S3Error> {
        let num_backends = self.backends.len();
        tracing::info!(
            "PUT object (multi sync streaming): streaming to {} backends",
            num_backends
        );

        let backends: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .map(|(idx, backend)| (idx, Arc::clone(backend)))
            .collect();

        let results = Self::broadcast_stream_to_backends(backends, key, body).await?;

        // Check that all backends succeeded
        let mut etags = vec![String::new(); num_backends];
        for (idx, result) in results {
            match result {
                Ok(etag) => {
                    tracing::info!("Backend {} successfully wrote object", idx);
                    etags[idx] = etag;
                }
                Err(e) => {
                    tracing::error!("Backend {} failed to write object: {}", idx, e);
                    // TODO: Implement rollback - delete from successful backends
                    return Err(S3Error::InternalError(format!(
                        "Backend {} failed in multi sync mode: {}",
                        idx, e
                    )));
                }
            }
        }

        tracing::info!("PUT object (multi sync streaming): all backends succeeded");

        // Return primary etag
        Ok(etags[self.primary_index].clone())
    }

    /// Legacy method using full buffering (kept for reference, unused)
    #[allow(dead_code)]
    async fn put_object_multi_sync(&self, key: &str, data: Bytes) -> Result<String, S3Error> {
        // Write to all backends concurrently, all must succeed
        tracing::info!(
            "PUT object (multi sync): writing to {} backends (all must succeed)",
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
                    let stream = MultiBackend::bytes_to_stream(data);
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
                        "Backend {} failed in multi sync mode: {}",
                        idx, e
                    )));
                }
            }
        }

        tracing::info!("PUT object (multi sync): all backends succeeded");

        // Return primary etag
        Ok(etags[self.primary_index].clone())
    }

    /// Spawn background task that GETs from primary once and broadcasts to other backends
    fn spawn_background_replication_tasks_streaming(&self, key: &str) {
        if self.backends.len() <= 1 {
            return;
        }

        let primary_idx = self.primary_index;
        let primary_backend = Arc::clone(&self.backends[primary_idx]);
        let other_backends: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .filter(move |(idx, _)| *idx != primary_idx)
            .map(|(idx, backend)| (idx, Arc::clone(backend)))
            .collect();

        tracing::info!(
            "Spawning background task to replicate to {} other backends (streaming from primary)",
            other_backends.len()
        );

        let key_clone = key.to_string();
        tokio::spawn(async move {
            // GET from primary once
            let (stream, _metadata) = match primary_backend.get_object(&key_clone).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!(
                        "Background replication: failed to get object {} from primary: {}",
                        key_clone,
                        e
                    );
                    return;
                }
            };

            // Use shared broadcast function to stream to all other backends
            match MultiBackend::broadcast_stream_to_backends(other_backends, &key_clone, stream)
                .await
            {
                Ok(results) => {
                    for (idx, result) in results {
                        match result {
                            Ok(_) => {
                                tracing::info!(
                                    "Background replication: backend {} successfully wrote object {}",
                                    idx,
                                    key_clone
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Background replication: backend {} failed to write object {}: {}",
                                    idx,
                                    key_clone,
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Background replication: failed to broadcast stream for object {}: {}",
                        key_clone,
                        e
                    );
                }
            }
        });
    }

    /// Legacy method using full buffering (kept for reference, unused)
    #[allow(dead_code)]
    fn spawn_background_replication_tasks(&self, key: &str, data: Bytes) {
        if self.backends.len() <= 1 {
            return;
        }

        let primary_idx = self.primary_index;
        let other_backends: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .filter(move |(idx, _)| *idx != primary_idx)
            .map(|(idx, backend)| (idx, Arc::clone(backend)))
            .collect();

        if other_backends.is_empty() {
            return;
        }

        tracing::info!(
            "Spawning background tasks to replicate to {} other backends",
            other_backends.len()
        );

        let key_clone = key.to_string();
        tokio::spawn(async move {
            for (idx, backend) in other_backends {
                let backend_clone = Arc::clone(&backend);
                let key = key_clone.clone();
                let data = data.clone();

                tokio::spawn(async move {
                    let stream = MultiBackend::bytes_to_stream(data);
                    match backend_clone.put_object(&key, stream).await {
                        Ok(_) => {
                            tracing::info!(
                                "Background replication: backend {} successfully wrote object {}",
                                idx,
                                key
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "Background replication: backend {} failed to write object {}: {}",
                                idx,
                                key,
                                e
                            );
                        }
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ReadMode;
    use crate::storage::{InMemoryStorage, backend::StorageBackend};
    use bytes::Bytes;
    use futures::stream::{self, StreamExt};

    // Helper function to convert Bytes to ObjectStream for tests
    fn bytes_to_stream(data: Bytes) -> crate::storage::backend::ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }

    #[tokio::test]
    async fn test_multibackend_put_and_get_multi_sync() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            0, // backend1 is primary (index 0)
            ReadMode::PrimaryFallback,
            WriteMode::MultiSync,
        );

        let key = "test-key";
        let data = Bytes::from("Hello, World!");

        // Put object (synchronous to all backends)
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

        // Verify both backends have the object (guaranteed with MultiSync)
        assert!(backend1.head_object(key).await.is_ok());
        assert!(backend2.head_object(key).await.is_ok());
    }

    #[tokio::test]
    async fn test_multibackend_put_consistent_mode() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            0, // first backend as primary
            ReadMode::PrimaryOnly,
            WriteMode::MultiSync,
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
}
