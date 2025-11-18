use crate::{
    app_state::AppState,
    types::{error::S3Error, AuthContext},
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};
use bytes::Bytes;

/// PUT /{bucket_name}/{key} - Put an object
pub async fn put_object(
    Path(key): Path<String>,
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
    _headers: HeaderMap,
    body: Bytes,
) -> Result<Response, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!(
        "PUT object: bucket={}, key={}, size={}",
        bucket,
        key,
        body.len()
    );

    // Store the object
    let etag = storage.put_object(&key, body).await?;

    // Return success with ETag
    Ok((StatusCode::OK, [("etag", etag)]).into_response())
}
