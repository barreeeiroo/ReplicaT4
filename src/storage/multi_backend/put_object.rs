use super::MultiBackend;
use crate::config::WriteMode;
use crate::storage::backend::ObjectStream;
use crate::types::error::S3Error;
use std::sync::Arc;

impl MultiBackend {
    pub(super) async fn put_object_impl(
        &self,
        key: &str,
        body: ObjectStream,
    ) -> Result<String, S3Error> {
        // Collect the stream into memory first (needed for replication)
        tracing::debug!("PUT object: collecting stream for replication");
        let data = Self::collect_stream(body).await?;

        tracing::debug!("PUT object: collected {} bytes", data.len());

        match self.write_mode {
            WriteMode::AsyncReplication => {
                // Write to primary backend immediately
                let primary = self.primary();
                tracing::info!(
                    "PUT object (async replication): writing to primary backend immediately"
                );

                let stream = Self::bytes_to_stream(data.clone());
                let etag = primary.put_object(key, stream).await?;

                tracing::info!("Primary backend successfully wrote object {}", key);

                // Spawn background tasks to replicate to other backends
                if self.backends.len() > 1 {
                    let primary_idx = self.primary_index;
                    let other_backends: Vec<_> = self
                        .backends
                        .iter()
                        .enumerate()
                        .filter(move |(idx, _)| *idx != primary_idx)
                        .map(|(idx, backend)| (idx, Arc::clone(backend)))
                        .collect();

                    if !other_backends.is_empty() {
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

                Ok(etag)
            }
            WriteMode::MultiSync => {
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
                            tracing::error!(
                                "Backend {} failed to write object {}: {}",
                                idx,
                                key,
                                e
                            );
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
        }
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
