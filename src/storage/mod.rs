mod backend;
mod in_memory;
mod multi_backend;
mod s3;

pub use backend::StorageBackend;
pub use in_memory::InMemoryStorage;
pub use multi_backend::{MultiBackend, determine_primary_by_latency};
pub use s3::S3Backend;
