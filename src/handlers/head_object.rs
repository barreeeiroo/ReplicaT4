use crate::{
    app_state::AppState,
    types::{error::S3Error, AuthContext},
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};

/// HEAD /{bucket_name}/{key} - Get object metadata
pub async fn head_object(
    Path(key): Path<String>,
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Response, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!("HEAD object: bucket={}, key={}", bucket, key);

    let metadata = storage.head_object(&key).await?;

    // Return headers only (no body)
    Ok((
        StatusCode::OK,
        [
            ("content-type", metadata.content_type),
            ("etag", metadata.etag),
            (
                "last-modified",
                metadata.last_modified.to_rfc2822().replace("+0000", "GMT"),
            ),
            ("content-length", metadata.size.to_string()),
        ],
    )
        .into_response())
}
