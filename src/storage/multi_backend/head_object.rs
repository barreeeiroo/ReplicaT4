use super::MultiBackend;
use crate::config::ReadMode;
use crate::types::{ObjectMetadata, error::S3Error};

impl MultiBackend {
    pub(super) async fn head_object_impl(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        match self.read_mode {
            ReadMode::PrimaryOnly => self.head_object_primary_only(key).await,
            ReadMode::PrimaryFallback => self.head_object_primary_fallback(key).await,
            ReadMode::BestEffort => self.head_object_best_effort(key).await,
        }
    }

    async fn head_object_primary_only(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        // Only read from primary backend
        tracing::debug!("HEAD object (primary only mode)");
        self.primary().head_object(key).await
    }

    async fn head_object_primary_fallback(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        // Try primary first, then fallback to others
        let primary = self.primary();
        tracing::debug!("HEAD object (trying primary backend first)");
        match primary.head_object(key).await {
            Ok(metadata) => return Ok(metadata),
            Err(S3Error::NoSuchKey) => {
                // Don't fallback if key doesn't exist in primary
                tracing::debug!("Key {} not found in primary backend, not falling back", key);
                return Err(S3Error::NoSuchKey);
            }
            Err(e) => {
                tracing::warn!("Primary backend failed for HEAD {}: {}", key, e);
            }
        }

        // Try other backends (only if there was a real error, not NoSuchKey)
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

    async fn head_object_best_effort(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        // Try all backends concurrently, return first success
        tracing::debug!(
            "HEAD object (best effort mode - racing {} backends)",
            self.backends.len()
        );

        use futures::future::FutureExt;
        use std::sync::Arc;

        let tasks: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .map(|(idx, backend)| {
                let backend = Arc::clone(backend);
                let key = key.to_string();
                async move {
                    let result = backend.head_object(&key).await;
                    (idx, result)
                }
                .boxed()
            })
            .collect();

        use futures::stream::{FuturesUnordered, StreamExt};
        let mut futures = tasks.into_iter().collect::<FuturesUnordered<_>>();

        let mut last_error = None;
        while let Some((idx, result)) = futures.next().await {
            match result {
                Ok(metadata) => {
                    tracing::debug!("Backend {} won the race and returned object metadata", idx);
                    // Explicitly drop remaining futures to cancel them
                    drop(futures);
                    return Ok(metadata);
                }
                Err(S3Error::NoSuchKey) => {
                    tracing::debug!("Backend {} returned NoSuchKey (valid response)", idx);
                    // NoSuchKey is a valid response, not an error - return immediately
                    drop(futures);
                    return Err(S3Error::NoSuchKey);
                }
                Err(e) => {
                    tracing::debug!("Backend {} failed in race for HEAD {}: {}", idx, key, e);
                    // Real error - continue trying other backends
                    last_error = Some(e);
                }
            }
        }

        // All backends failed with real errors
        Err(last_error.unwrap_or(S3Error::NoSuchKey))
    }
}
