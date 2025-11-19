use crate::types::Credentials;
use std::collections::HashMap;
use std::sync::Arc;

/// Credentials store - maps access key ID to credentials
#[derive(Clone)]
pub struct CredentialsStore {
    credentials: Arc<HashMap<String, Credentials>>,
}

impl CredentialsStore {
    /// Create a new credentials store from a map of access key IDs to credentials
    pub fn new(credentials: HashMap<String, Credentials>) -> Self {
        Self {
            credentials: Arc::new(credentials),
        }
    }

    /// Get credentials for a given access key ID
    /// Returns None if the access key ID is not found
    pub fn get(&self, access_key_id: &str) -> Option<&Credentials> {
        self.credentials.get(access_key_id)
    }
}
