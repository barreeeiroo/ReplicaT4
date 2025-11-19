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
}
