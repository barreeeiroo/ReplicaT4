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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth::CredentialsStore, storage::InMemoryStorage, types::Credentials};
    use axum::{
        Router,
        body::Body,
        http::{Method, StatusCode},
        middleware::from_fn,
        routing::get,
    };
    use std::collections::HashMap;
    use tower::util::ServiceExt;

    // Helper function to create test app state
    fn create_test_app_state() -> AppState {
        let mut credentials = HashMap::new();
        credentials.insert(
            "AKIAIOSFODNN7EXAMPLE".to_string(),
            Credentials {
                _access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            },
        );
        let credentials_store = CredentialsStore::new(credentials);
        let storage: std::sync::Arc<dyn crate::storage::StorageBackend> =
            std::sync::Arc::new(InMemoryStorage::new());
        AppState::new(storage, credentials_store, "test-bucket".to_string())
    }

    // Helper handler that succeeds if auth passed
    async fn test_handler() -> &'static str {
        "OK"
    }

    // Helper to create router with middleware
    fn create_test_router(app_state: AppState) -> Router {
        Router::new()
            .route("/test", get(test_handler))
            .layer(from_fn(move |request: Request, next: Next| {
                let state = app_state.clone();
                async move { auth_middleware(state, request, next).await }
            }))
    }

    #[tokio::test]
    async fn test_missing_authorization_header() {
        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_invalid_authorization_header() {
        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .header("authorization", "InvalidFormat")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_unknown_access_key() {
        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .header(
                "authorization",
                "AWS4-HMAC-SHA256 Credential=UNKNOWNKEY/20240101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abc123"
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_malformed_credential() {
        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .header(
                "authorization",
                "AWS4-HMAC-SHA256 Credential=INVALIDFORMAT, SignedHeaders=host, Signature=abc",
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_signature_verification_failure() {
        use chrono::Utc;

        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = &timestamp[..8];

        // Valid format but wrong signature
        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .header("host", "example.com")
            .header("x-amz-date", &timestamp)
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .header(
                "authorization",
                format!(
                    "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/{}/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=wrongsignature",
                    date
                )
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should fail signature verification
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_missing_required_header_x_amz_content_sha256() {
        use chrono::Utc;

        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = &timestamp[..8];

        // Missing x-amz-content-sha256 header
        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .header("host", "example.com")
            .header("x-amz-date", &timestamp)
            .header(
                "authorization",
                format!(
                    "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/{}/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abc123",
                    date
                )
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should fail due to missing required header
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_expired_timestamp_in_middleware() {
        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        // Very old timestamp
        let old_timestamp = "20200101T000000Z";

        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .header("host", "example.com")
            .header("x-amz-date", old_timestamp)
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .header(
                "authorization",
                "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20200101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abc123"
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should fail due to expired timestamp
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_missing_signed_headers() {
        use chrono::Utc;

        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = &timestamp[..8];

        // Missing SignedHeaders field
        let request = Request::builder()
            .uri("/test")
            .method(Method::GET)
            .header("host", "example.com")
            .header("x-amz-date", &timestamp)
            .header(
                "authorization",
                format!(
                    "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/{}/us-east-1/s3/aws4_request, Signature=abc123",
                    date
                )
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_successful_authentication() {
        use chrono::Utc;
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let app_state = create_test_app_state();
        let app = create_test_router(app_state);

        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = &timestamp[..8];

        // Build a proper request with valid signature
        let method = "GET";
        let uri = "/test";
        let host = "example.com";
        let content_hash = "UNSIGNED-PAYLOAD";

        // Build canonical request
        let canonical_headers = format!("host:{}\nx-amz-date:{}\n", host, timestamp);
        let signed_headers = "host;x-amz-date";
        let canonical_request = format!(
            "{}\n{}\n\n{}\n{}\n{}",
            method, uri, canonical_headers, signed_headers, content_hash
        );

        // Build string to sign
        let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let credential_scope = format!("{}/us-east-1/s3/aws4_request", date);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            timestamp, credential_scope, hashed_canonical_request
        );

        // Calculate signature
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

        type HmacSha256 = Hmac<Sha256>;

        let k_secret = format!("AWS4{}", secret_key);
        let mut mac = HmacSha256::new_from_slice(k_secret.as_bytes()).unwrap();
        mac.update(date.as_bytes());
        let k_date = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&k_date).unwrap();
        mac.update(b"us-east-1");
        let k_region = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&k_region).unwrap();
        mac.update(b"s3");
        let k_service = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&k_service).unwrap();
        mac.update(b"aws4_request");
        let k_signing = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&k_signing).unwrap();
        mac.update(string_to_sign.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        // Build request with valid signature
        let request = Request::builder()
            .uri(uri)
            .method(method)
            .header("host", host)
            .header("x-amz-date", &timestamp)
            .header("x-amz-content-sha256", content_hash)
            .header(
                "authorization",
                format!(
                    "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/{}, SignedHeaders={}, Signature={}",
                    credential_scope, signed_headers, signature
                )
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should succeed and reach the handler
        assert_eq!(response.status(), StatusCode::OK);

        // Verify response body
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body_bytes, "OK");
    }
}
