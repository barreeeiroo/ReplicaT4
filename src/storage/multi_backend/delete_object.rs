use super::MultiBackend;
use crate::config::WriteMode;
use crate::types::error::S3Error;
use std::sync::Arc;

impl MultiBackend {
    pub(super) async fn delete_object_impl(&self, key: &str) -> Result<(), S3Error> {
        match self.write_mode {
            WriteMode::AsyncReplication => {
                // Delete from primary backend immediately
                let primary = self.primary();
                tracing::info!(
                    "DELETE object (async replication): deleting from primary backend immediately"
                );

                primary.delete_object(key).await?;
                tracing::info!("Primary backend successfully deleted object {}", key);

                // Spawn background tasks to delete from other backends
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
                            "Spawning background tasks to delete from {} other backends",
                            other_backends.len()
                        );

                        let key_clone = key.to_string();
                        tokio::spawn(async move {
                            for (idx, backend) in other_backends {
                                let backend_clone = Arc::clone(&backend);
                                let key = key_clone.clone();

                                tokio::spawn(async move {
                                    match backend_clone.delete_object(&key).await {
                                        Ok(_) => {
                                            tracing::info!(
                                                "Background deletion: backend {} successfully deleted object {}",
                                                idx,
                                                key
                                            );
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Background deletion: backend {} failed to delete object {}: {}",
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

                Ok(())
            }
            WriteMode::MultiSync => {
                // Delete from all backends concurrently, all must succeed
                tracing::info!(
                    "DELETE object (multi sync): deleting from {} backends (all must succeed)",
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
                            tracing::error!(
                                "Backend {} failed to delete object {}: {}",
                                idx,
                                key,
                                e
                            );
                            return Err(S3Error::InternalError(format!(
                                "Backend {} failed to delete in multi sync mode: {}",
                                idx, e
                            )));
                        }
                    }
                }

                tracing::info!("DELETE object (multi sync): all backends succeeded");
                Ok(())
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
    use futures::stream;

    // Helper function to convert Bytes to ObjectStream for tests
    fn bytes_to_stream(data: Bytes) -> crate::storage::backend::ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }

    #[tokio::test]
    async fn test_multibackend_delete_best_effort() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            0, // first backend as primary
            ReadMode::PrimaryFallback,
            WriteMode::AsyncReplication,
        );

        let key = "test-key";
        let data = Bytes::from("data");

        // Put and then delete
        multi.put_object(key, bytes_to_stream(data)).await.unwrap();
        multi.delete_object(key).await.unwrap();

        // Verify both backends deleted the object
        assert!(backend1.head_object(key).await.is_err());
        assert!(backend2.head_object(key).await.is_err());
    }
}
