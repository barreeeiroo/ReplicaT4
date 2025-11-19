use super::MultiBackend;
use crate::types::{error::S3Error, ObjectMetadata};

impl MultiBackend {
    pub(super) async fn list_objects_impl(
        &self,
        prefix: Option<&str>,
        max_keys: i32,
    ) -> Result<Vec<ObjectMetadata>, S3Error> {
        // Query primary backend
        let backend = self.primary();
        tracing::debug!("LIST objects (using primary backend)");
        backend.list_objects(prefix, max_keys).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ReadMode, WriteMode};
    use crate::storage::{backend::StorageBackend, InMemoryStorage};
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
