use crate::{app_state::AppState, types::{error::S3Error, AuthContext}};
use super::signature::{parse_authorization_header, verify_signature};
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// AWS Signature V4 authentication middleware
/// Note: app_state must be captured in a closure when creating the middleware layer
pub async fn auth_middleware(
    app_state: AppState,
    mut request: Request,
    next: Next,
) -> Response {
    // Extract the Authorization header
    let auth_header = match request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
    {
        Some(h) => h,
        None => return S3Error::AccessDenied.into_response(),
    };

    // Parse the authorization header
    let auth_info = match parse_authorization_header(auth_header) {
        Ok(info) => info,
        Err(e) => return e.into_response(),
    };

    // Look up the credentials by access key ID
    let credentials = match app_state.credentials.get(&auth_info.access_key_id) {
        Some(c) => c,
        None => return S3Error::AccessDenied.into_response(),
    };

    // Verify the signature
    if let Err(e) = verify_signature(&request, &auth_info, credentials) {
        return e.into_response();
    }

    // Insert auth context into request extensions for downstream handlers
    request.extensions_mut().insert(AuthContext {
        _access_key_id: auth_info.access_key_id.clone(),
    });

    // Continue to the next middleware/handler
    next.run(request).await
}
