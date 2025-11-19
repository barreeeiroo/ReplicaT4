use crate::{
    app_state::AppState,
    types::{AuthContext, error::S3Error},
};
use axum::{
    Extension,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// HEAD /{bucket_name} - Check if bucket exists and user has access
pub async fn head_bucket(
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Response, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!("HEAD bucket: bucket={}", bucket);

    storage.head_bucket().await?;

    // Return 200 OK with no body
    Ok(StatusCode::OK.into_response())
}
