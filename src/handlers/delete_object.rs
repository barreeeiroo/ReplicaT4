use crate::{
    app_state::AppState,
    types::{AuthContext, error::S3Error},
};
use axum::{
    Extension,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

/// DELETE /{bucket_name}/{key} - Delete an object
pub async fn delete_object(
    Path(key): Path<String>,
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!("DELETE object: bucket={}, key={}", bucket, key);

    storage.delete_object(&key).await?;

    // S3 returns 204 No Content on successful delete
    Ok(StatusCode::NO_CONTENT)
}
