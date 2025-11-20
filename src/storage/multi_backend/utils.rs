use super::MultiBackend;
use crate::storage::backend::StorageBackend;
use crate::types::error::S3Error;
use std::sync::Arc;

impl MultiBackend {
    /// Helper: Try primary backend first, fallback to others on real errors (not NoSuchKey/NoSuchBucket)
    pub(super) async fn try_primary_fallback<F, Fut, T>(
        &self,
        operation_name: &str,
        is_not_found_error: fn(&S3Error) -> bool,
        not_found_error: S3Error,
        operation: F,
    ) -> Result<T, S3Error>
    where
        F: Fn(Arc<dyn StorageBackend>) -> Fut,
        Fut: std::future::Future<Output = Result<T, S3Error>>,
    {
        let primary = self.primary();
        tracing::debug!("{} (trying primary backend first)", operation_name);

        match operation(Arc::clone(primary)).await {
            Ok(result) => return Ok(result),
            Err(e) if is_not_found_error(&e) => {
                tracing::debug!("Primary backend returned not found, not falling back");
                return Err(e);
            }
            Err(e) => {
                tracing::warn!("Primary backend failed for {}: {}", operation_name, e);
            }
        }

        // Try other backends
        for (idx, backend) in self.other_backends().enumerate() {
            tracing::debug!("{} (trying fallback backend {})", operation_name, idx);
            match operation(Arc::clone(backend)).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        "Fallback backend {} failed for {}: {}",
                        idx,
                        operation_name,
                        e
                    );
                }
            }
        }

        Err(not_found_error)
    }

    /// Helper: Race all backends concurrently, return first success
    pub(super) async fn race_all_backends<F, Fut, T>(
        &self,
        operation_name: &str,
        is_not_found_error: fn(&S3Error) -> bool,
        not_found_error: S3Error,
        operation: F,
    ) -> Result<T, S3Error>
    where
        F: Fn(Arc<dyn StorageBackend>) -> Fut,
        Fut: std::future::Future<Output = Result<T, S3Error>> + Send + 'static,
        T: Send + 'static,
    {
        use futures::future::FutureExt;
        use futures::stream::{FuturesUnordered, StreamExt};

        tracing::debug!(
            "{} (best effort mode - racing {} backends)",
            operation_name,
            self.backends.len()
        );

        let tasks: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .map(|(idx, backend)| {
                let backend = Arc::clone(backend);
                let fut = operation(backend);
                async move {
                    let result = fut.await;
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
                    tracing::debug!("Backend {} won the race", idx);
                    drop(futures);
                    return Ok(data);
                }
                Err(e) if is_not_found_error(&e) => {
                    tracing::debug!("Backend {} returned not found (valid response)", idx);
                    drop(futures);
                    return Err(e);
                }
                Err(e) => {
                    tracing::debug!("Backend {} failed in race: {}", idx, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(not_found_error))
    }

    /// Helper: Verify all backends return consistent ETags
    pub(super) async fn verify_all_consistent_etag<F, Fut, T>(
        &self,
        operation_name: &str,
        extract_etag: fn(&T) -> &str,
        operation: F,
    ) -> Result<T, S3Error>
    where
        F: Fn(Arc<dyn StorageBackend>) -> Fut,
        Fut: Future<Output = Result<T, S3Error>> + Send + 'static,
        T: Send + 'static,
    {
        use futures::future::FutureExt;

        tracing::debug!(
            "{} (all consistent mode - verifying {} backends)",
            operation_name,
            self.backends.len()
        );

        let tasks: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .map(|(idx, backend)| {
                let backend = Arc::clone(backend);
                let fut = operation(backend);
                async move {
                    let result = fut.await;
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
                Ok(data) => {
                    successful_results.push((idx, data));
                }
                Err(e) => {
                    tracing::warn!("Backend {} failed for {}: {}", idx, operation_name, e);
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
        let primary_etag = extract_etag(&successful_results[self.primary_index].1);
        for (idx, data) in &successful_results {
            let etag = extract_etag(data);
            if etag != primary_etag {
                tracing::error!(
                    "ETag mismatch: backend {} has {}, primary has {}",
                    idx,
                    etag,
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

    /// Helper: Verify all backends succeed (no ETag checking)
    pub(super) async fn verify_all_succeed<F, Fut, T>(
        &self,
        operation_name: &str,
        operation: F,
    ) -> Result<T, S3Error>
    where
        F: Fn(Arc<dyn StorageBackend>) -> Fut,
        Fut: std::future::Future<Output = Result<T, S3Error>> + Send + 'static,
        T: Send + 'static,
    {
        use futures::future::FutureExt;

        tracing::debug!(
            "{} (all consistent mode - verifying {} backends)",
            operation_name,
            self.backends.len()
        );

        let tasks: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .map(|(idx, backend)| {
                let backend = Arc::clone(backend);
                let fut = operation(backend);
                async move {
                    let result = fut.await;
                    (idx, result)
                }
                .boxed()
            })
            .collect();

        let results = futures::future::join_all(tasks).await;

        // All backends must succeed
        let mut primary_result = None;
        for (idx, result) in results {
            match result {
                Ok(data) => {
                    if idx == self.primary_index {
                        primary_result = Some(data);
                    }
                }
                Err(e) => {
                    tracing::error!("Backend {} failed for {}: {}", idx, operation_name, e);
                    return Err(S3Error::InternalError(format!(
                        "Consistency check failed: backend {} failed",
                        idx
                    )));
                }
            }
        }

        tracing::debug!("All backends succeeded for {}", operation_name);
        Ok(primary_result.unwrap())
    }
}
