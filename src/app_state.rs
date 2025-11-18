use crate::{auth::CredentialsStore, storage::StorageBackend};
use std::sync::Arc;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageBackend>,
    pub credentials: CredentialsStore,
    pub bucket_name: String,
}

impl AppState {
    pub fn new(
        storage: Arc<dyn StorageBackend>,
        credentials: CredentialsStore,
        bucket_name: String,
    ) -> Self {
        Self {
            storage,
            credentials,
            bucket_name,
        }
    }
}
