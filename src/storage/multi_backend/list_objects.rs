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
            ReadMode::AllConsistent => self.list_objects_all_consistent(prefix, max_keys).await,
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

    async fn list_objects_all_consistent(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        // Fetch from all backends and verify lists match
        tracing::debug!(
            "LIST objects (all consistent mode - verifying {} backends)",
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

        let results = futures::future::join_all(tasks).await;

        // Collect all successful results
        let mut all_lists = Vec::new();

        for (idx, result) in results {
            match result {
                Ok(objects) => {
                    all_lists.push((idx, objects));
                }
                Err(e) => {
                    tracing::warn!("Backend {} failed for LIST objects: {}", idx, e);
                    return Err(S3Error::InternalError(format!(
                        "Consistency check failed: backend {} failed",
                        idx
                    )));
                }
            }
        }

        // All backends must succeed
        if all_lists.len() != self.backends.len() {
            return Err(S3Error::InternalError(format!(
                "Consistency check failed: only {}/{} backends succeeded",
                all_lists.len(),
                self.backends.len()
            )));
        }

        // Verify all lists have the same objects (same keys and ETags)
        use std::collections::HashMap;

        let primary_list = &all_lists[self.primary_index].1;
        let primary_map: HashMap<_, _> = primary_list
            .iter()
            .map(|obj| (&obj.key, &obj.etag))
            .collect();

        for (idx, objects) in &all_lists {
            if *idx == self.primary_index {
                continue;
            }

            // Check same number of objects
            if objects.len() != primary_list.len() {
                tracing::error!(
                    "List length mismatch: backend {} has {} objects, primary has {}",
                    idx,
                    objects.len(),
                    primary_list.len()
                );
                return Err(S3Error::InternalError(
                    "Consistency check failed: different number of objects".to_string(),
                ));
            }

            // Check each object has same ETag
            for obj in objects {
                match primary_map.get(&obj.key) {
                    Some(primary_etag) => {
                        if &obj.etag != *primary_etag {
                            tracing::error!(
                                "ETag mismatch for key {}: backend {} has {}, primary has {}",
                                obj.key,
                                idx,
                                obj.etag,
                                primary_etag
                            );
                            return Err(S3Error::InternalError(
                                "Consistency check failed: ETag mismatch".to_string(),
                            ));
                        }
                    }
                    None => {
                        tracing::error!(
                            "Key {} exists in backend {} but not in primary",
                            obj.key,
                            idx
                        );
                        return Err(S3Error::InternalError(
                            "Consistency check failed: different objects".to_string(),
                        ));
                    }
                }
            }
        }

        tracing::debug!(
            "All backends returned consistent lists ({} objects)",
            primary_list.len()
        );

        // Return primary result
        Ok(all_lists.remove(self.primary_index).1)
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
