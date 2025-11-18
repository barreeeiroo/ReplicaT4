use crate::types::Credentials;
use std::collections::HashMap;
use std::sync::Arc;

/// Credentials store - maps access key ID to credentials
#[derive(Clone)]
pub struct CredentialsStore {
    credentials: Arc<HashMap<String, Credentials>>,
}

impl CredentialsStore {
    pub fn new(credentials: HashMap<String, Credentials>) -> Self {
        Self {
            credentials: Arc::new(credentials),
        }
    }

    pub fn get(&self, access_key_id: &str) -> Option<&Credentials> {
        self.credentials.get(access_key_id)
    }
}
