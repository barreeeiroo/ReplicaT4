use super::MultiBackend;
use crate::types::error::S3Error;

impl MultiBackend {
    pub(super) async fn head_bucket_impl(&self) -> Result<(), S3Error> {
        // Query primary backend
        let backend = self.primary();
        tracing::debug!("HEAD bucket (using primary backend)");
        backend.head_bucket().await
    }
}
