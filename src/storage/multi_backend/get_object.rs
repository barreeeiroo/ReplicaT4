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
            ReadMode::AllConsistent => self.get_object_all_consistent(key).await,
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
        let key = key.to_string();
        self.try_primary_fallback(
            "GET object",
            |e| matches!(e, S3Error::NoSuchKey),
            S3Error::NoSuchKey,
            |backend| {
                let key = key.clone();
                async move { backend.get_object(&key).await }
            },
        )
        .await
    }

    async fn get_object_best_effort(
        &self,
        key: &str,
    ) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        let key = key.to_string();
        self.race_all_backends(
            "GET object",
            |e| matches!(e, S3Error::NoSuchKey),
            S3Error::NoSuchKey,
            |backend| {
                let key = key.clone();
                async move { backend.get_object(&key).await }
            },
        )
        .await
    }

    async fn get_object_all_consistent(
        &self,
        key: &str,
    ) -> Result<(ObjectStream, ObjectMetadata), S3Error> {
        let key = key.to_string();
        self.verify_all_consistent_etag(
            "GET object",
            |result: &(ObjectStream, ObjectMetadata)| &result.1.etag,
            |backend| {
                let key = key.clone();
                async move { backend.get_object(&key).await }
            },
        )
        .await
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
