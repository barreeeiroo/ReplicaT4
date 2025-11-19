use crate::types::{Credentials, error::S3Error};
use axum::{body::Body, extract::Request, http::HeaderMap};

/// Parsed authorization header information
#[derive(Debug)]
pub struct AuthorizationInfo {
    pub access_key_id: String,
    pub credential_scope: String,
    pub signed_headers: Vec<String>,
    pub signature: String,
}

/// Parse the AWS4-HMAC-SHA256 authorization header
///
/// Extracts authentication components from an AWS Signature Version 4 Authorization header.
/// Expected format: `AWS4-HMAC-SHA256 Credential=..., SignedHeaders=..., Signature=...`
///
/// Returns AuthorizationInfo containing access key ID, credential scope, signed headers, and signature.
/// Returns InvalidRequest error if the header format is invalid or missing required components.
pub fn parse_authorization_header(header: &str) -> Result<AuthorizationInfo, S3Error> {
    // Format: AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,
    //         SignedHeaders=host;range;x-amz-date, Signature=fe5f80f77d5fa3beca038a248ff027d0445342fe2855ddc963176630326f1024

    if !header.starts_with("AWS4-HMAC-SHA256 ") {
        return Err(S3Error::InvalidRequest(
            "Invalid authorization header format".to_string(),
        ));
    }

    let parts = header.trim_start_matches("AWS4-HMAC-SHA256 ");
    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;

    for part in parts.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("Credential=") {
            credential = Some(value);
        } else if let Some(value) = part.strip_prefix("SignedHeaders=") {
            signed_headers = Some(value);
        } else if let Some(value) = part.strip_prefix("Signature=") {
            signature = Some(value);
        }
    }

    let credential = credential.ok_or_else(|| {
        S3Error::InvalidRequest("Missing Credential in authorization header".to_string())
    })?;

    let signed_headers = signed_headers.ok_or_else(|| {
        S3Error::InvalidRequest("Missing SignedHeaders in authorization header".to_string())
    })?;

    let signature = signature.ok_or_else(|| {
        S3Error::InvalidRequest("Missing Signature in authorization header".to_string())
    })?;

    // Parse credential: AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request
    let credential_parts: Vec<&str> = credential.split('/').collect();
    if credential_parts.len() != 5 {
        return Err(S3Error::InvalidRequest(
            "Invalid credential format".to_string(),
        ));
    }

    let access_key_id = credential_parts[0].to_string();
    let credential_scope = credential_parts[1..].join("/");

    let signed_headers_vec: Vec<String> =
        signed_headers.split(';').map(|s| s.to_string()).collect();

    Ok(AuthorizationInfo {
        access_key_id,
        credential_scope,
        signed_headers: signed_headers_vec,
        signature: signature.to_string(),
    })
}

/// Verify the AWS Signature V4
///
/// Validates that the signature in the authorization header matches the expected signature
/// calculated from the request using the provided credentials.
///
/// This function:
/// 1. Extracts required headers (x-amz-date, x-amz-content-sha256)
/// 2. Validates the timestamp (allows 15 minute clock skew)
/// 3. Builds the canonical request
/// 4. Creates the string to sign
/// 5. Calculates the expected signature using HMAC-SHA256
/// 6. Compares with the provided signature
///
/// Returns Ok(()) if signature is valid, or SignatureDoesNotMatch/InvalidRequest error otherwise.
pub fn verify_signature(
    request: &Request<Body>,
    auth_info: &AuthorizationInfo,
    credentials: &Credentials,
) -> Result<(), S3Error> {
    // Extract required headers
    let headers = request.headers();

    // Get date: prefer x-amz-date, fall back to Date header
    let amz_date = headers
        .get("x-amz-date")
        .or_else(|| headers.get("date"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| S3Error::InvalidRequest("Missing x-amz-date or date header".to_string()))?;

    let content_hash = headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            S3Error::InvalidRequest("Missing x-amz-content-sha256 header".to_string())
        })?;

    // Validate timestamp (allow 15 minute skew)
    validate_timestamp(amz_date)?;

    // Build canonical request
    let canonical_request =
        build_canonical_request(request, &auth_info.signed_headers, content_hash)?;

    // Build string to sign
    let string_to_sign =
        build_string_to_sign(&canonical_request, amz_date, &auth_info.credential_scope);

    // Calculate signature
    let calculated_signature =
        calculate_signature(&credentials.secret_access_key, amz_date, &string_to_sign)?;

    // Compare signatures
    if calculated_signature != auth_info.signature {
        tracing::warn!(
            "Signature mismatch. Expected: {}, Got: {}",
            calculated_signature,
            auth_info.signature
        );
        return Err(S3Error::SignatureDoesNotMatch);
    }

    Ok(())
}

/// Validate that the timestamp is within acceptable range
fn validate_timestamp(date_str: &str) -> Result<(), S3Error> {
    use chrono::{DateTime, Duration, NaiveDateTime, Utc};

    // Try parsing as x-amz-date format first (20251118T195229Z)
    let request_time = if date_str.ends_with('Z') && date_str.len() == 16 {
        // Parse ISO 8601 basic format: YYYYMMDDTHHMMSSZ
        let date_part = &date_str[..15]; // Remove the 'Z'
        NaiveDateTime::parse_from_str(date_part, "%Y%m%dT%H%M%S")
            .map(|dt| dt.and_utc())
            .map_err(|e| {
                tracing::warn!("Failed to parse x-amz-date format: {}", e);
                S3Error::InvalidRequest(format!("Invalid x-amz-date format: {}", e))
            })?
    } else {
        // Fall back to RFC 2822 format (used by Date header)
        DateTime::parse_from_rfc2822(date_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                tracing::warn!("Failed to parse date format: {}", e);
                S3Error::InvalidRequest(format!("Invalid date format: {}", e))
            })?
    };

    let now = Utc::now();
    let diff = now.signed_duration_since(request_time);

    // Allow 15 minute skew in either direction
    if diff > Duration::minutes(15) || diff < Duration::minutes(-15) {
        return Err(S3Error::InvalidRequest(
            "Request timestamp too skewed".to_string(),
        ));
    }

    Ok(())
}

/// Build the canonical request string
fn build_canonical_request(
    request: &Request<Body>,
    signed_headers: &[String],
    content_hash: &str,
) -> Result<String, S3Error> {
    // HTTP method
    let method = request.method().as_str();

    // Canonical URI (path)
    let uri = request.uri().path();

    // Canonical query string (must be sorted and properly encoded)
    let raw_query = request.uri().query().unwrap_or("");
    let canonical_query = canonicalize_query_string(raw_query);

    // Canonical headers
    let canonical_headers = build_canonical_headers(request.headers(), signed_headers);

    // Signed headers list
    let signed_headers_str = signed_headers.join(";");

    // Canonical request format:
    // HTTPMethod + '\n' +
    // CanonicalURI + '\n' +
    // CanonicalQueryString + '\n' +
    // CanonicalHeaders + '\n' +
    // SignedHeaders + '\n' +
    // HashedPayload

    Ok(format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, uri, canonical_query, canonical_headers, signed_headers_str, content_hash
    ))
}

/// Canonicalize query string according to AWS SigV4 spec
/// Parameters must be sorted by name. The query string we receive is already percent-encoded,
/// so we preserve that encoding and just sort the parameters.
fn canonicalize_query_string(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }

    // Parse query parameters - keep them as-is (already percent-encoded)
    let mut params: Vec<(&str, &str)> = query
        .split('&')
        .map(|param| {
            if let Some((key, value)) = param.split_once('=') {
                (key, value)
            } else {
                // Handle key without value
                (param, "")
            }
        })
        .collect();

    // Sort by key name (then by value if keys are equal)
    params.sort_by(|a, b| match a.0.cmp(b.0) {
        std::cmp::Ordering::Equal => a.1.cmp(b.1),
        other => other,
    });

    // Build canonical query string - use values as-is since they're already encoded
    params
        .iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>()
        .join("&")
}

/// Build canonical headers string
fn build_canonical_headers(headers: &HeaderMap, signed_headers: &[String]) -> String {
    let mut canonical = String::new();

    for header_name in signed_headers {
        if let Some(value) = headers.get(header_name) {
            if let Ok(value_str) = value.to_str() {
                let trimmed = value_str.trim();
                canonical.push_str(header_name);
                canonical.push(':');
                canonical.push_str(trimmed);
                canonical.push('\n');
            }
        } else {
            tracing::warn!(
                "Signed header '{}' not found in request headers",
                header_name
            );
        }
    }

    canonical
}

/// Build the string to sign
fn build_string_to_sign(canonical_request: &str, amz_date: &str, credential_scope: &str) -> String {
    use sha2::{Digest, Sha256};

    let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, credential_scope, hashed_canonical_request
    )
}

/// Calculate the signature
fn calculate_signature(
    secret_key: &str,
    amz_date: &str,
    string_to_sign: &str,
) -> Result<String, S3Error> {
    // Extract date from amz_date (first 8 characters: YYYYMMDD)
    let date = &amz_date[..8];

    // Derive signing key
    let k_secret = format!("AWS4{}", secret_key);
    let k_date = hmac_sha256(k_secret.as_bytes(), date.as_bytes())?;
    let k_region = hmac_sha256(&k_date, b"us-east-1")?; // TODO: make region configurable
    let k_service = hmac_sha256(&k_region, b"s3")?;
    let k_signing = hmac_sha256(&k_service, b"aws4_request")?;

    // Calculate signature
    let signature = hmac_sha256(&k_signing, string_to_sign.as_bytes())?;

    Ok(hex::encode(signature))
}

/// HMAC-SHA256 helper
fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<Vec<u8>, S3Error> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| S3Error::InternalError(format!("HMAC error: {}", e)))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_authorization_header() {
        let header = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=host;x-amz-date,Signature=abc123";
        let result = parse_authorization_header(header);
        assert!(result.is_ok());

        let auth_info = result.unwrap();
        assert_eq!(auth_info.access_key_id, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(
            auth_info.credential_scope,
            "20130524/us-east-1/s3/aws4_request"
        );
        assert_eq!(auth_info.signed_headers, vec!["host", "x-amz-date"]);
        assert_eq!(auth_info.signature, "abc123");
    }

    #[test]
    fn test_parse_missing_prefix() {
        let header = "Credential=KEY/scope";
        assert!(matches!(
            parse_authorization_header(header),
            Err(S3Error::InvalidRequest(_))
        ));
    }

    #[test]
    fn test_parse_missing_credential() {
        let header = "AWS4-HMAC-SHA256 SignedHeaders=host,Signature=abc";
        assert!(matches!(
            parse_authorization_header(header),
            Err(S3Error::InvalidRequest(_))
        ));
    }

    #[test]
    fn test_canonicalize_query_string() {
        assert_eq!(canonicalize_query_string(""), "");
        assert_eq!(canonicalize_query_string("foo=bar"), "foo=bar");

        // Test sorting
        let result = canonicalize_query_string("z=1&a=2");
        assert_eq!(result, "a=2&z=1");
    }

    #[test]
    fn test_parse_missing_signed_headers() {
        let header = "AWS4-HMAC-SHA256 Credential=KEY/a/b/c/d,Signature=abc";
        assert!(matches!(
            parse_authorization_header(header),
            Err(S3Error::InvalidRequest(_))
        ));
    }

    #[test]
    fn test_parse_missing_signature() {
        let header = "AWS4-HMAC-SHA256 Credential=KEY/a/b/c/d,SignedHeaders=host";
        assert!(matches!(
            parse_authorization_header(header),
            Err(S3Error::InvalidRequest(_))
        ));
    }

    #[test]
    fn test_parse_invalid_credential_format() {
        let header = "AWS4-HMAC-SHA256 Credential=INVALIDFORMAT,SignedHeaders=host,Signature=abc";
        assert!(matches!(
            parse_authorization_header(header),
            Err(S3Error::InvalidRequest(_))
        ));
    }

    #[test]
    fn test_parse_with_whitespace() {
        let header = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abc123";
        let result = parse_authorization_header(header);
        assert!(result.is_ok());

        let auth_info = result.unwrap();
        assert_eq!(auth_info.access_key_id, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(auth_info.signature, "abc123");
    }

    #[test]
    fn test_canonicalize_query_string_multiple_values() {
        let result = canonicalize_query_string("foo=bar&foo=baz");
        assert_eq!(result, "foo=bar&foo=baz");
    }

    #[test]
    fn test_canonicalize_query_string_empty_value() {
        let result = canonicalize_query_string("foo=");
        assert_eq!(result, "foo=");
    }

    #[test]
    fn test_canonicalize_query_string_no_value() {
        let result = canonicalize_query_string("foo");
        assert_eq!(result, "foo=");
    }

    #[test]
    fn test_canonicalize_query_string_encoded() {
        let result = canonicalize_query_string("key=hello%20world");
        assert_eq!(result, "key=hello%20world");
    }

    #[test]
    fn test_canonicalize_query_string_complex_sorting() {
        let result = canonicalize_query_string("z=1&a=3&a=2&b=1");
        assert_eq!(result, "a=2&a=3&b=1&z=1");
    }

    #[test]
    fn test_build_canonical_headers() {
        use axum::http::HeaderMap;

        let mut headers = HeaderMap::new();
        headers.insert("host", "example.com".parse().unwrap());
        headers.insert("x-amz-date", "20240101T120000Z".parse().unwrap());

        let signed_headers = vec!["host".to_string(), "x-amz-date".to_string()];
        let canonical = build_canonical_headers(&headers, &signed_headers);

        assert!(canonical.contains("host:example.com\n"));
        assert!(canonical.contains("x-amz-date:20240101T120000Z\n"));
    }

    #[test]
    fn test_build_canonical_headers_trimming() {
        use axum::http::HeaderMap;

        let mut headers = HeaderMap::new();
        headers.insert("host", "  example.com  ".parse().unwrap());

        let signed_headers = vec!["host".to_string()];
        let canonical = build_canonical_headers(&headers, &signed_headers);

        assert_eq!(canonical, "host:example.com\n");
    }

    #[test]
    fn test_build_canonical_headers_missing_header() {
        use axum::http::HeaderMap;

        let headers = HeaderMap::new();
        let signed_headers = vec!["host".to_string()];
        let canonical = build_canonical_headers(&headers, &signed_headers);

        // Should handle missing headers gracefully
        assert_eq!(canonical, "");
    }

    #[test]
    fn test_build_string_to_sign() {
        let canonical_request = "GET\n/\n\nhost:example.com\n\nhost\nHASH";
        let amz_date = "20240101T120000Z";
        let credential_scope = "20240101/us-east-1/s3/aws4_request";

        let string_to_sign = build_string_to_sign(canonical_request, amz_date, credential_scope);

        assert!(string_to_sign.starts_with("AWS4-HMAC-SHA256\n"));
        assert!(string_to_sign.contains(amz_date));
        assert!(string_to_sign.contains(credential_scope));
    }

    #[test]
    fn test_hmac_sha256() {
        let key = b"key";
        let data = b"data";
        let result = hmac_sha256(key, data);

        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_calculate_signature() {
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let amz_date = "20240101T120000Z";
        let string_to_sign =
            "AWS4-HMAC-SHA256\n20240101T120000Z\n20240101/us-east-1/s3/aws4_request\nabc123";

        let result = calculate_signature(secret_key, amz_date, string_to_sign);

        assert!(result.is_ok());
        let signature = result.unwrap();
        assert_eq!(signature.len(), 64); // SHA256 hex is 64 characters
    }

    #[test]
    fn test_validate_timestamp_valid() {
        use chrono::Utc;

        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();

        let result = validate_timestamp(&timestamp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_timestamp_invalid_format() {
        let result = validate_timestamp("invalid");
        assert!(matches!(result, Err(S3Error::InvalidRequest(_))));
    }

    #[test]
    fn test_validate_timestamp_too_old() {
        // A timestamp from 30 minutes ago (should fail with 15 minute tolerance)
        let old_timestamp = "20200101T000000Z";
        let result = validate_timestamp(old_timestamp);
        assert!(matches!(result, Err(S3Error::InvalidRequest(_))));
    }

    #[test]
    fn test_build_canonical_request_simple() {
        use axum::http::{Method, Request};

        let request = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/test")
            .header("host", "example.com")
            .header("x-amz-date", "20240101T120000Z")
            .body(Body::empty())
            .unwrap();

        let signed_headers = vec!["host".to_string(), "x-amz-date".to_string()];
        let content_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"; // Empty body hash

        let result = build_canonical_request(&request, &signed_headers, content_hash);
        assert!(result.is_ok());

        let canonical = result.unwrap();
        assert!(canonical.starts_with("GET\n"));
        assert!(canonical.contains("/test\n"));
        assert!(canonical.contains("host:example.com\n"));
        assert!(canonical.contains("x-amz-date:20240101T120000Z\n"));
        assert!(canonical.contains("\nhost;x-amz-date\n"));
        assert!(canonical.ends_with(content_hash));
    }

    #[test]
    fn test_build_canonical_request_with_query() {
        use axum::http::{Method, Request};

        let request = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/test?foo=bar&baz=qux")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();

        let signed_headers = vec!["host".to_string()];
        let content_hash = "UNSIGNED-PAYLOAD";

        let result = build_canonical_request(&request, &signed_headers, content_hash);
        assert!(result.is_ok());

        let canonical = result.unwrap();
        // Query parameters should be sorted
        assert!(canonical.contains("baz=qux&foo=bar"));
    }

    #[test]
    fn test_build_canonical_request_post() {
        use axum::http::{Method, Request};

        let request = Request::builder()
            .method(Method::POST)
            .uri("https://example.com/object")
            .header("host", "example.com")
            .header("content-type", "text/plain")
            .body(Body::empty())
            .unwrap();

        let signed_headers = vec!["content-type".to_string(), "host".to_string()];
        let content_hash = "abc123";

        let result = build_canonical_request(&request, &signed_headers, content_hash);
        assert!(result.is_ok());

        let canonical = result.unwrap();
        assert!(canonical.starts_with("POST\n"));
        assert!(canonical.contains("content-type:text/plain\n"));
    }

    #[test]
    fn test_build_canonical_request_empty_query() {
        use axum::http::{Method, Request};

        let request = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/test")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();

        let signed_headers = vec!["host".to_string()];
        let content_hash = "HASH";

        let result = build_canonical_request(&request, &signed_headers, content_hash);
        assert!(result.is_ok());

        let canonical = result.unwrap();
        // Should have empty query string section
        assert!(canonical.contains("/test\n\n")); // URI followed by empty query
    }

    #[test]
    fn test_verify_signature_missing_date_header() {
        use axum::http::{Method, Request};

        let request = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/test")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();

        let auth_info = AuthorizationInfo {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            credential_scope: "20240101/us-east-1/s3/aws4_request".to_string(),
            signed_headers: vec!["host".to_string()],
            signature: "abc123".to_string(),
        };

        let credentials = Credentials {
            _access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
        };

        let result = verify_signature(&request, &auth_info, &credentials);
        assert!(matches!(result, Err(S3Error::InvalidRequest(_))));
    }

    #[test]
    fn test_verify_signature_missing_content_hash() {
        use axum::http::{Method, Request};
        use chrono::Utc;

        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();

        let request = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/test")
            .header("host", "example.com")
            .header("x-amz-date", &timestamp)
            .body(Body::empty())
            .unwrap();

        let auth_info = AuthorizationInfo {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            credential_scope: format!("{}/us-east-1/s3/aws4_request", &timestamp[..8]),
            signed_headers: vec!["host".to_string(), "x-amz-date".to_string()],
            signature: "abc123".to_string(),
        };

        let credentials = Credentials {
            _access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
        };

        let result = verify_signature(&request, &auth_info, &credentials);
        assert!(matches!(result, Err(S3Error::InvalidRequest(_))));
    }

    #[test]
    fn test_verify_signature_invalid_signature() {
        use axum::http::{Method, Request};
        use chrono::Utc;

        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();

        let request = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/test")
            .header("host", "example.com")
            .header("x-amz-date", &timestamp)
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .body(Body::empty())
            .unwrap();

        let auth_info = AuthorizationInfo {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            credential_scope: format!("{}/us-east-1/s3/aws4_request", &timestamp[..8]),
            signed_headers: vec!["host".to_string(), "x-amz-date".to_string()],
            signature: "invalidsignature123".to_string(),
        };

        let credentials = Credentials {
            _access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
        };

        let result = verify_signature(&request, &auth_info, &credentials);
        // Should fail because signature doesn't match
        assert!(matches!(result, Err(S3Error::SignatureDoesNotMatch)));
    }

    #[test]
    fn test_verify_signature_expired_timestamp() {
        use axum::http::{Method, Request};

        // Timestamp from 30 minutes ago (beyond 15 minute tolerance)
        let old_timestamp = "20200101T000000Z";

        let request = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/test")
            .header("host", "example.com")
            .header("x-amz-date", old_timestamp)
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .body(Body::empty())
            .unwrap();

        let auth_info = AuthorizationInfo {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            credential_scope: "20200101/us-east-1/s3/aws4_request".to_string(),
            signed_headers: vec!["host".to_string(), "x-amz-date".to_string()],
            signature: "abc123".to_string(),
        };

        let credentials = Credentials {
            _access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
        };

        let result = verify_signature(&request, &auth_info, &credentials);
        // Should fail due to timestamp validation
        assert!(matches!(result, Err(S3Error::InvalidRequest(_))));
    }
}
