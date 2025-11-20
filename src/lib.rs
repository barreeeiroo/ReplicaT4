// Library exports for integration tests
pub mod app_state;
pub mod auth;
pub mod config;
pub mod handlers;
pub mod server;
pub mod storage;
pub mod types;

// Re-export commonly used types
pub use app_state::AppState;
pub use auth::CredentialsStore;
pub use config::{BackendConfig, Config};
pub use storage::{InMemoryStorage, MultiBackend, S3Backend, StorageBackend};
pub use types::Credentials;

// Re-export server creation function
pub use server::create_app;
