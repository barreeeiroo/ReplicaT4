use super::signature::{parse_authorization_header, verify_signature};
use crate::{
    app_state::AppState,
    types::{AuthContext, error::S3Error},
};
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// AWS Signature V4 authentication middleware
///
/// Validates incoming requests using AWS Signature Version 4 authentication.
/// This middleware:
/// 1. Extracts and parses the Authorization header
/// 2. Looks up credentials by access key ID
/// 3. Verifies the signature matches the expected value
/// 4. Injects AuthContext into request extensions for downstream handlers
///
/// Returns AccessDenied or SignatureDoesNotMatch errors if authentication fails.
///
/// Note: app_state must be captured in a closure when creating the middleware layer
pub async fn auth_middleware(app_state: AppState, mut request: Request, next: Next) -> Response {
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
