use crate::{
    app_state::AppState,
    types::{AuthContext, error::S3Error},
};
use axum::{
    Extension,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::stream::StreamExt;

/// PUT /{bucket_name}/{key} - Put an object
pub async fn put_object(
    Path(key): Path<String>,
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
    _headers: HeaderMap,
    body: Body,
) -> Result<Response, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!("PUT object: bucket={}, key={}", bucket, key);

    // Convert axum Body to ObjectStream
    let stream = body.into_data_stream().map(|result| {
        result.map_err(|e| S3Error::InternalError(format!("Failed to read body: {}", e)))
    });
    let boxed_stream = Box::pin(stream);

    // Store the object
    let etag = storage.put_object(&key, boxed_stream).await?;

    // Return success with ETag
    Ok((StatusCode::OK, [("etag", etag)]).into_response())
}
