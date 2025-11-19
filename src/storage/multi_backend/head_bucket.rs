use super::MultiBackend;
use crate::config::ReadMode;
use crate::types::error::S3Error;
use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;

impl MultiBackend {
    pub(super) async fn head_bucket_impl(&self) -> Result<(), S3Error> {
        match self.read_mode {
            ReadMode::PrimaryOnly => self.head_bucket_primary_only().await,
            ReadMode::PrimaryFallback => self.head_bucket_primary_fallback().await,
            ReadMode::BestEffort => self.head_bucket_best_effort().await,
        }
    }

    async fn head_bucket_primary_only(&self) -> Result<(), S3Error> {
        // Only check primary backend
        tracing::debug!("HEAD bucket (primary only mode)");
        self.primary().head_bucket().await
    }

    async fn head_bucket_primary_fallback(&self) -> Result<(), S3Error> {
        // Try primary first, then fallback to others
        let primary = self.primary();
        tracing::debug!("HEAD bucket (trying primary backend first)");
        match primary.head_bucket().await {
            Ok(()) => return Ok(()),
            Err(S3Error::NoSuchBucket) => {
                // Don't fallback if bucket doesn't exist in primary
                tracing::debug!("Bucket not found in primary backend, not falling back");
                return Err(S3Error::NoSuchBucket);
            }
            Err(e) => {
                tracing::warn!("Primary backend failed for HEAD bucket: {}", e);
            }
        }

        // Try other backends (only if there was a real error, not NoSuchBucket)
        for (idx, backend) in self.other_backends().enumerate() {
            tracing::debug!("HEAD bucket (trying fallback backend {})", idx);
            match backend.head_bucket().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!("Fallback backend {} failed for HEAD bucket: {}", idx, e);
                }
            }
        }

        Err(S3Error::NoSuchBucket)
    }

    async fn head_bucket_best_effort(&self) -> Result<(), S3Error> {
        // Try all backends concurrently, return first success
        tracing::debug!(
            "HEAD bucket (best effort mode - racing {} backends)",
            self.backends.len()
        );

        let tasks: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .map(|(idx, backend)| {
                let backend = Arc::clone(backend);
                async move {
                    let result = backend.head_bucket().await;
                    (idx, result)
                }
                .boxed()
            })
            .collect();

        let mut futures = tasks.into_iter().collect::<FuturesUnordered<_>>();

        let mut last_error = None;
        while let Some((idx, result)) = futures.next().await {
            match result {
                Ok(()) => {
                    tracing::debug!("Backend {} won the race and confirmed bucket exists", idx);
                    // Explicitly drop remaining futures to cancel them
                    drop(futures);
                    return Ok(());
                }
                Err(S3Error::NoSuchBucket) => {
                    tracing::debug!("Backend {} returned NoSuchBucket (valid response)", idx);
                    // NoSuchBucket is a valid response, not an error - return immediately
                    drop(futures);
                    return Err(S3Error::NoSuchBucket);
                }
                Err(e) => {
                    tracing::debug!("Backend {} failed in race for HEAD bucket: {}", idx, e);
                    // Real error - continue trying other backends
                    last_error = Some(e);
                }
            }
        }

        // All backends failed with real errors
        Err(last_error.unwrap_or(S3Error::NoSuchBucket))
    }
}
