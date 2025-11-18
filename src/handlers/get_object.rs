use crate::{
    app_state::AppState,
    types::{AuthContext, error::S3Error},
};
use axum::{
    Extension,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// GET /{bucket_name}/{key} - Get an object
pub async fn get_object(
    Path(key): Path<String>,
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Response, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!("GET object: bucket={}, key={}", bucket, key);

    // Retrieve object from storage
    let data = storage.get_object(&key).await?;
    let metadata = storage.head_object(&key).await?;

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
        data,
    )
        .into_response())
}
