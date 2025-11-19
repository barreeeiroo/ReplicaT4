use super::MultiBackend;
use crate::config::ReadMode;
use crate::types::{ObjectMetadata, error::S3Error};

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
        let key = key.to_string();
        self.race_all_backends(
            "HEAD object",
            |e| matches!(e, S3Error::NoSuchKey),
            S3Error::NoSuchKey,
            |backend| {
                let key = key.clone();
                async move { backend.head_object(&key).await }
            },
        )
        .await
    }

    async fn head_object_all_consistent(&self, key: &str) -> Result<ObjectMetadata, S3Error> {
        let key = key.to_string();
        self.verify_all_consistent_etag(
            "HEAD object",
            |metadata: &ObjectMetadata| &metadata.etag,
            |backend| {
                let key = key.clone();
                async move { backend.head_object(&key).await }
            },
        )
        .await
    }
}
