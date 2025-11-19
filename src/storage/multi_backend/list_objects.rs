use super::MultiBackend;
use crate::config::ReadMode;
use crate::types::{ObjectMetadata, error::S3Error};
use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;

impl MultiBackend {
    pub(super) async fn list_objects_impl(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        match self.read_mode {
            ReadMode::PrimaryOnly => self.list_objects_primary_only(prefix, max_keys).await,
            ReadMode::PrimaryFallback => self.list_objects_primary_fallback(prefix, max_keys).await,
            ReadMode::BestEffort => self.list_objects_best_effort(prefix, max_keys).await,
        }
    }

    async fn list_objects_primary_only(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        // Only list from primary backend
        tracing::debug!("LIST objects (primary only mode)");
        self.primary().list_objects(prefix, max_keys).await
    }

    async fn list_objects_primary_fallback(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        // Try primary first, then fallback to others
        let primary = self.primary();
        tracing::debug!("LIST objects (trying primary backend first)");
        match primary.list_objects(prefix, max_keys).await {
            Ok(objects) => return Ok(objects),
            Err(e) => {
                tracing::warn!("Primary backend failed for LIST objects: {}", e);
            }
        }

        // Try other backends (on any error - even an empty list is valid, not an error)
        for (idx, backend) in self.other_backends().enumerate() {
            tracing::debug!("LIST objects (trying fallback backend {})", idx);
            match backend.list_objects(prefix, max_keys).await {
                Ok(objects) => return Ok(objects),
                Err(e) => {
                    tracing::warn!("Fallback backend {} failed for LIST objects: {}", idx, e);
                }
            }
        }

        Err(S3Error::InternalError("All backends failed".to_string()))
    }

    async fn list_objects_best_effort(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        // Try all backends concurrently, return first success
        tracing::debug!(
            "LIST objects (best effort mode - racing {} backends)",
            self.backends.len()
        );

        let tasks: Vec<_> = self
            .backends
            .iter()
            .enumerate()
            .map(|(idx, backend)| {
                let backend = Arc::clone(backend);
                let prefix = prefix.map(|s| s.to_string());
                async move {
                    let result = backend.list_objects(prefix.as_deref(), max_keys).await;
                    (idx, result)
                }
                .boxed()
            })
            .collect();

        let mut futures = tasks.into_iter().collect::<FuturesUnordered<_>>();

        let mut last_error = None;
        while let Some((idx, result)) = futures.next().await {
            match result {
                Ok(objects) => {
                    tracing::debug!("Backend {} won the race and returned list", idx);
                    // Explicitly drop remaining futures to cancel them
                    drop(futures);
                    return Ok(objects);
                }
                Err(e) => {
                    tracing::debug!("Backend {} failed in race for LIST objects: {}", idx, e);
                    // Real error - continue trying other backends
                    last_error = Some(e);
                }
            }
        }

        // All backends failed with real errors
        Err(last_error.unwrap_or(S3Error::InternalError("All backends failed".to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ReadMode, WriteMode};
    use crate::storage::{InMemoryStorage, backend::StorageBackend};
    use bytes::Bytes;
    use futures::stream;
    use std::sync::Arc;

    // Helper function to convert Bytes to ObjectStream for tests
    fn bytes_to_stream(data: Bytes) -> crate::storage::backend::ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }

    #[tokio::test]
    async fn test_multibackend_list_objects() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            0, // backend1 is primary
            ReadMode::PrimaryFallback,
            WriteMode::AsyncReplication,
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
