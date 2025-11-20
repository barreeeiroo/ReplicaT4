use super::backend::{ObjectStream, StorageBackend};
use crate::config::{ReadMode, WriteMode};
use crate::types::{ObjectMetadata, error::S3Error};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod delete_object;
mod get_object;
mod head_bucket;
mod head_object;
mod list_objects;
mod put_object;
mod utils;

/// Multi-backend storage that replicates operations across multiple backends
pub struct MultiBackend {
    pub(super) backends: Vec<Arc<dyn StorageBackend>>,
    pub(super) primary_index: usize,
    pub(super) read_mode: ReadMode,
    pub(super) write_mode: WriteMode,
}

impl MultiBackend {
    /// Create a new multi-backend storage
    ///
    /// # Arguments
    /// * `backends` - List of storage backends to use (must be non-empty)
    /// * `primary_index` - Index of the primary backend (0-based)
    /// * `read_mode` - Read consistency mode
    /// * `write_mode` - Write consistency mode
    pub fn new(
        backends: Vec<Arc<dyn StorageBackend>>,
        primary_index: usize,
        read_mode: ReadMode,
        write_mode: WriteMode,
    ) -> Self {
        tracing::info!(
            "Initializing MultiBackend with {} backends (primary_index: {}, read_mode: {:?}, write_mode: {:?})",
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

    /// Get the primary backend
    pub(super) fn primary(&self) -> &Arc<dyn StorageBackend> {
        &self.backends[self.primary_index]
    }

    /// Get all backends except the primary (for fallback reads)
    pub(super) fn other_backends(&self) -> impl Iterator<Item = &Arc<dyn StorageBackend>> {
        let primary_idx = self.primary_index;
        self.backends
            .iter()
            .enumerate()
            .filter(move |(idx, _)| *idx != primary_idx)
            .map(|(_, backend)| backend)
    }
}

/// Determine the best backend based on latency by benchmarking HEAD bucket requests
///
/// Issues 10 HEAD bucket requests to each backend and calculates the median (P50) latency.
/// Returns the index of the backend with the lowest P50 latency.
///
/// # Arguments
/// * `backends` - List of backends to benchmark
/// * `backend_names` - Names of the backends (for logging)
///
/// # Returns
/// The index of the backend with the best (lowest) P50 latency, or 0 if all backends fail
pub async fn determine_primary_by_latency(
    backends: &[Arc<dyn StorageBackend>],
    backend_names: &[String],
) -> usize {
    const BENCHMARK_REQUESTS: usize = 10;

    tracing::info!(
        "Benchmarking {} backends with {} HEAD bucket requests each...",
        backends.len(),
        BENCHMARK_REQUESTS
    );

    let mut backend_latencies: Vec<(usize, Vec<Duration>)> = Vec::new();

    // Benchmark each backend
    for (idx, backend) in backends.iter().enumerate() {
        let mut latencies = Vec::new();
        let backend_name = &backend_names[idx];

        tracing::debug!("Benchmarking backend '{}' ({})...", backend_name, idx);

        for request_num in 0..BENCHMARK_REQUESTS {
            let start = Instant::now();
            match backend.head_bucket().await {
                Ok(_) => {
                    let duration = start.elapsed();
                    latencies.push(duration);
                    tracing::trace!(
                        "Backend '{}' request {}/{}: {:?}",
                        backend_name,
                        request_num + 1,
                        BENCHMARK_REQUESTS,
                        duration
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Backend '{}' failed HEAD bucket request {}/{}: {}",
                        backend_name,
                        request_num + 1,
                        BENCHMARK_REQUESTS,
                        e
                    );
                }
            }
        }

        if latencies.is_empty() {
            tracing::warn!("Backend '{}' failed all benchmark requests", backend_name);
        } else {
            tracing::debug!(
                "Backend '{}' completed {}/{} requests",
                backend_name,
                latencies.len(),
                BENCHMARK_REQUESTS
            );
        }

        backend_latencies.push((idx, latencies));
    }

    // Calculate P50 (median) for each backend
    let mut backend_p50s: Vec<(usize, Option<Duration>)> = backend_latencies
        .into_iter()
        .map(|(idx, mut latencies)| {
            if latencies.is_empty() {
                (idx, None)
            } else {
                latencies.sort();
                let p50 = latencies[latencies.len() / 2];
                (idx, Some(p50))
            }
        })
        .collect();

    // Log P50 results
    for (idx, p50) in &backend_p50s {
        match p50 {
            Some(duration) => tracing::info!(
                "Backend '{}' P50 latency: {:?}",
                backend_names[*idx],
                duration
            ),
            None => tracing::warn!(
                "Backend '{}' has no successful requests",
                backend_names[*idx]
            ),
        }
    }

    // Find backend with lowest P50
    backend_p50s.sort_by(|a, b| match (a.1, b.1) {
        (Some(a_p50), Some(b_p50)) => a_p50.cmp(&b_p50),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let best_index = backend_p50s[0].0;
    if let Some(best_p50) = backend_p50s[0].1 {
        tracing::info!(
            "Selected backend '{}' as primary based on best P50 latency: {:?}",
            backend_names[best_index],
            best_p50
        );
    } else {
        tracing::warn!(
            "All backends failed benchmarking, defaulting to first backend '{}'",
            backend_names[0]
        );
        return 0;
    }

    best_index
}

#[async_trait::async_trait]
impl StorageBackend for MultiBackend {
    async fn head_bucket(&self) -> Result<(), S3Error> {
        self.head_bucket_impl().await
    }

    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        self.list_objects_impl(prefix, max_keys).await
    }

    async fn head_object(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        self.head_object_impl(key).await
    }

    async fn get_object(&self, key: &str) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        self.get_object_impl(key).await
    }

    async fn put_object(&self, key: &str, body: ObjectStream) -> Result<String, S3Error> {
        self.put_object_impl(key, body).await
    }

    async fn delete_object(&self, key: &str) -> Result<(), S3Error> {
        self.delete_object_impl(key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ReadMode, WriteMode};
    use crate::storage::InMemoryStorage;

    #[tokio::test]
    async fn test_multibackend_primary_selection() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            1, // backend2 is primary (index 1)
            ReadMode::PrimaryOnly,
            WriteMode::MultiSync,
        );

        assert_eq!(multi.primary_index, 1);
    }

    #[tokio::test]
    async fn test_determine_primary_by_latency() {
        // Create multiple in-memory backends
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend3 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let backends = vec![backend1, backend2, backend3];
        let backend_names = vec![
            "backend1".to_string(),
            "backend2".to_string(),
            "backend3".to_string(),
        ];

        // All backends should succeed, so one of them will be selected
        let primary_index = determine_primary_by_latency(&backends, &backend_names).await;

        // The result should be a valid index (0, 1, or 2)
        assert!(primary_index < 3);
    }

    #[tokio::test]
    async fn test_determine_primary_by_latency_all_backends_succeed() {
        // Create two in-memory backends
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let backends = vec![backend1, backend2];
        let backend_names = vec!["fast".to_string(), "slow".to_string()];

        // Both backends should succeed, and one will be chosen based on latency
        let primary_index = determine_primary_by_latency(&backends, &backend_names).await;

        // Should return a valid index
        assert!(primary_index < 2);
    }
}
