use super::MultiBackend;
use crate::config::ReadMode;
use crate::types::{ObjectMetadata, error::S3Error};
use futures::future::FutureExt;
use std::sync::Arc;

impl MultiBackend {
    pub(super) async fn head_object_impl(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        match self.read_mode {
            ReadMode::PrimaryOnly => self.head_object_primary_only(key).await,
            ReadMode::PrimaryFallback => self.head_object_primary_fallback(key).await,
            ReadMode::BestEffort => self.head_object_best_effort(key).await,
            ReadMode::AllConsistent => self.head_object_all_consistent(key).await,
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

    async fn head_object_all_consistent(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        // Fetch metadata from all backends and verify ETags match
        tracing::debug!(
            "HEAD object (all consistent mode - verifying {} backends)",
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
                    let result = backend.head_object(&key).await;
                    (idx, result)
                }
                .boxed()
            })
            .collect();

        let results = futures::future::join_all(tasks).await;

        // Collect all successful results
        let mut successful_results = Vec::new();

        for (idx, result) in results {
            match result {
                Ok(metadata) => {
                    successful_results.push((idx, metadata));
                }
                Err(e) => {
                    tracing::warn!("Backend {} failed for HEAD {}: {}", idx, key, e);
                }
            }
        }

        // All backends must succeed
        if successful_results.len() != self.backends.len() {
            return Err(S3Error::InternalError(format!(
                "Consistency check failed: only {}/{} backends succeeded",
                successful_results.len(),
                self.backends.len()
            )));
        }

        // Verify all ETags match
        let primary_etag = &successful_results[self.primary_index].1.etag;
        for (idx, metadata) in &successful_results {
            if &metadata.etag != primary_etag {
                tracing::error!(
                    "ETag mismatch: backend {} has {}, primary has {}",
                    idx,
                    metadata.etag,
                    primary_etag
                );
                return Err(S3Error::InternalError(
                    "Consistency check failed: ETag mismatch between backends".to_string(),
                ));
            }
        }

        tracing::debug!("All backends returned consistent ETags: {}", primary_etag);

        // Return primary result
        Ok(successful_results.remove(self.primary_index).1)
    }
}
