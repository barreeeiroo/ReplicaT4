use super::MultiBackend;
use crate::config::ReadMode;
use crate::types::error::S3Error;

impl MultiBackend {
    pub(super) async fn head_bucket_impl(&self) -> Result<(), S3Error> {
        match self.read_mode {
            ReadMode::PrimaryOnly => self.head_bucket_primary_only().await,
            ReadMode::PrimaryFallback => self.head_bucket_primary_fallback().await,
            ReadMode::BestEffort => self.head_bucket_best_effort().await,
            ReadMode::AllConsistent => self.head_bucket_all_consistent().await,
        }
    }

    async fn head_bucket_primary_only(&self) -> Result<(), S3Error> {
        // Only check primary backend
        tracing::debug!("HEAD bucket (primary only mode)");
        self.primary().head_bucket().await
    }

    async fn head_bucket_primary_fallback(&self) -> Result<(), S3Error> {
        self.try_primary_fallback(
            "HEAD bucket",
            |e| matches!(e, S3Error::NoSuchBucket),
            S3Error::NoSuchBucket,
            |backend| async move { backend.head_bucket().await },
        )
        .await
    }

    async fn head_bucket_best_effort(&self) -> Result<(), S3Error> {
        self.race_all_backends(
            "HEAD bucket",
            |e| matches!(e, S3Error::NoSuchBucket),
            S3Error::NoSuchBucket,
            |backend| async move { backend.head_bucket().await },
        )
        .await
    }

    async fn head_bucket_all_consistent(&self) -> Result<(), S3Error> {
        self.verify_all_succeed("HEAD bucket", |backend| async move {
            backend.head_bucket().await
        })
        .await
    }
}
