use super::MultiBackend;
use crate::config::ReadMode;
use crate::storage::backend::ObjectStream;
use crate::types::{ObjectMetadata, error::S3Error};
use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;

impl MultiBackend {
    pub(super) async fn get_object_impl(
        &self,
        key: &str,
    ) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        match self.read_mode {
            ReadMode::PrimaryOnly => self.get_object_primary_only(key).await,
            ReadMode::PrimaryFallback => self.get_object_primary_fallback(key).await,
            ReadMode::BestEffort => self.get_object_best_effort(key).await,
        }
    }

    async fn get_object_primary_only(
        &self,
        key: &str,
    ) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        // Only read from primary backend
        tracing::debug!("GET object (primary only mode)");
        self.primary().get_object(key).await
    }

    async fn get_object_primary_fallback(
        &self,
        key: &str,
    ) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        // Try primary first, then fallback to others
        let primary = self.primary();
        tracing::debug!("GET object (trying primary backend first)");
        match primary.get_object(key).await {
            Ok(result) => return Ok(result),
            Err(S3Error::NoSuchKey) => {
                // Don't fallback if key doesn't exist in primary
                tracing::debug!("Key {} not found in primary backend, not falling back", key);
                return Err(S3Error::NoSuchKey);
            }
            Err(e) => {
                tracing::warn!("Primary backend failed for GET {}: {}", key, e);
            }
        }

        // Try other backends (only if there was a real error, not NoSuchKey)
        for (idx, backend) in self.other_backends().enumerate() {
            tracing::debug!("GET object (trying fallback backend {})", idx);
            match backend.get_object(key).await {
                Ok(result) => return Ok(result),
                Err(S3Error::NoSuchKey) => {
                    // Don't fallback if key doesn't exist in any secondary
                    tracing::debug!(
                        "Key {} not found in fallback backend {}, not falling back",
                        key,
                        idx
                    );
                    return Err(S3Error::NoSuchKey);
                }
                Err(e) => {
                    tracing::warn!("Fallback backend {} failed for GET {}: {}", idx, key, e);
                }
            }
        }

        Err(S3Error::NoSuchKey)
    }

    async fn get_object_best_effort(
        &self,
        key: &str,
    ) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        // Try all backends concurrently, return first success
        tracing::debug!(
            "GET object (best effort mode - racing {} backends)",
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
                    let result = backend.get_object(&key).await;
                    (idx, result)
                }
                .boxed()
            })
            .collect();

        let mut futures = tasks.into_iter().collect::<FuturesUnordered<_>>();

        let mut last_error = None;
        while let Some((idx, result)) = futures.next().await {
            match result {
                Ok(data) => {
                    tracing::debug!("Backend {} won the race and returned object", idx);
                    // Explicitly drop remaining futures to cancel them
                    drop(futures);
                    return Ok(data);
                }
                Err(S3Error::NoSuchKey) => {
                    tracing::debug!("Backend {} returned NoSuchKey (valid response)", idx);
                    // NoSuchKey is a valid response, not an error - return immediately
                    drop(futures);
                    return Err(S3Error::NoSuchKey);
                }
                Err(e) => {
                    tracing::debug!("Backend {} failed in race for GET {}: {}", idx, key, e);
                    // Real error - continue trying other backends
                    last_error = Some(e);
                }
            }
        }

        // All backends failed with real errors
        Err(last_error.unwrap_or(S3Error::NoSuchKey))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WriteMode;
    use crate::storage::{InMemoryStorage, backend::StorageBackend};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_multibackend_get_no_such_key() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            0, // backend1 is primary
            ReadMode::PrimaryFallback,
            WriteMode::AsyncReplication,
        );

        let key = "nonexistent-key";

        // Get should return NoSuchKey without checking other backends
        let result = multi.get_object(key).await;
        assert!(matches!(result, Err(S3Error::NoSuchKey)));
    }
}
