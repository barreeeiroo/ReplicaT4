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
            Err(e) => {
                tracing::warn!("Primary backend failed for HEAD {}: {}", key, e);
            }
        }

        // Try other backends
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
        // Try all backends, return first success
        tracing::debug!("HEAD object (best effort mode - trying all backends)");
        for (idx, backend) in self.backends.iter().enumerate() {
            match backend.head_object(key).await {
                Ok(metadata) => {
                    tracing::debug!("Backend {} returned object metadata", idx);
                    return Ok(metadata);
                }
                Err(e) => {
                    tracing::debug!("Backend {} failed for HEAD {}: {}", idx, key, e);
                }
            }
        }

        Err(S3Error::NoSuchKey)
    }
}
