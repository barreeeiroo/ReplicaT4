use crate::{
    app_state::AppState,
    types::{AuthContext, error::S3Error},
};
use axum::{
    Extension,
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::TryStreamExt;

/// GET /{bucket_name}/{key} - Get an object
pub async fn get_object(
    Path(key): Path<String>,
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Response, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!("GET object: bucket={}, key={}", bucket, key);

    // Retrieve object stream and metadata from storage in a single call
    let (stream, metadata) = storage.get_object(&key).await?;

    // Convert our stream to axum Body
    // The stream yields Result<Bytes, S3Error>, we need to map errors to std::io::Error for Body
    let body_stream = stream.map_err(|e| std::io::Error::other(e.to_string()));
    let body = Body::from_stream(body_stream);

    // Build response with S3 headers
    Ok((
        StatusCode::OK,
        [
            ("content-type", metadata.content_type.clone()),
            ("etag", metadata.etag.clone()),
            (
                "last-modified",
                metadata.last_modified.to_rfc2822().replace("+0000", "GMT"),
            ),
            ("content-length", metadata.size.to_string()),
        ],
        body,
    )
        .into_response())
}
