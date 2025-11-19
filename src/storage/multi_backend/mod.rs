use super::backend::{ObjectStream, StorageBackend};
use crate::config::{ReadMode, WriteMode};
use crate::types::{error::S3Error, ObjectMetadata};
use bytes::{Bytes, BytesMut};
use futures::stream::{self, StreamExt};
use std::sync::Arc;

mod delete_object;
mod get_object;
mod head_bucket;
mod head_object;
mod list_objects;
mod put_object;

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

    /// Collect a stream into Bytes (needed for replication)
    pub(super) async fn collect_stream(mut stream: ObjectStream) -> Result<Bytes, S3Error> {
        let mut data = BytesMut::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk);
        }
        Ok(data.freeze())
    }

    /// Create a new stream from Bytes
    pub(super) fn bytes_to_stream(data: Bytes) -> ObjectStream {
        Box::pin(stream::once(async move { Ok(data) }))
    }
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
}
