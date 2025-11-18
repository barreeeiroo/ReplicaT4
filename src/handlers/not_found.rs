use crate::types::error::S3Error;
use axum::response::{IntoResponse, Response};

/// Fallback handler for 404 Not Found
/// Returns S3-compatible NoSuchBucket error in XML format
/// This is triggered when the route doesn't match our configured bucket structure
pub async fn not_found() -> Response {
    S3Error::NoSuchBucket.into_response()
}
