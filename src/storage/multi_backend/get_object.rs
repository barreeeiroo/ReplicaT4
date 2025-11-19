use super::MultiBackend;
use crate::config::ReadMode;
use crate::storage::backend::ObjectStream;
use crate::types::{ObjectMetadata, error::S3Error};

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
            Err(e) => {
                tracing::warn!("Primary backend failed for GET {}: {}", key, e);
            }
        }

        // Try other backends
        for (idx, backend) in self.other_backends().enumerate() {
            tracing::debug!("GET object (trying fallback backend {})", idx);
            match backend.get_object(key).await {
                Ok(result) => return Ok(result),
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
        // Try all backends, return first success
        tracing::debug!("GET object (best effort mode - trying all backends)");
        for (idx, backend) in self.backends.iter().enumerate() {
            match backend.get_object(key).await {
                Ok(result) => {
                    tracing::debug!("Backend {} returned object", idx);
                    return Ok(result);
                }
                Err(e) => {
                    tracing::debug!("Backend {} failed for GET {}: {}", idx, key, e);
                }
            }
        }

        Err(S3Error::NoSuchKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WriteMode;
    use crate::storage::{InMemoryStorage, backend::StorageBackend};
    use bytes::Bytes;
    use futures::stream::{self, StreamExt};
    use std::sync::Arc;

    // Helper function to convert Bytes to ObjectStream for tests
    fn bytes_to_stream(data: Bytes) -> crate::storage::backend::ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }

    #[tokio::test]
    async fn test_multibackend_read_fallback() {
        let backend1 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;
        let backend2 = Arc::new(InMemoryStorage::new()) as Arc<dyn StorageBackend>;

        let multi = MultiBackend::new(
            vec![backend1.clone(), backend2.clone()],
            0, // backend1 is primary
            ReadMode::PrimaryFallback,
            WriteMode::AsyncReplication,
        );

        let key = "test-key";
        let data = Bytes::from("fallback test");

        // Put object only in backend2 (simulate primary failure)
        backend2
            .put_object(key, bytes_to_stream(data.clone()))
            .await
            .unwrap();

        // Get should succeed by falling back to backend2
        let (mut stream, _) = multi.get_object(key).await.unwrap();

        let mut collected = Vec::new();
        while let Some(result) = stream.next().await {
            collected.extend_from_slice(&result.unwrap());
        }
        assert_eq!(collected, data);
    }
}
