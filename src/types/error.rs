use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quick_xml::se::to_string as to_xml_string;
use serde::Serialize;

/// S3-compatible error responses
#[derive(Debug)]
pub enum S3Error {
    NoSuchKey,
    NoSuchBucket,
    InvalidRequest(String),
    AccessDenied,
    SignatureDoesNotMatch,
    InternalError(String),
}

/// S3 XML error response format
#[derive(Serialize)]
#[serde(rename = "Error")]
struct S3ErrorResponse {
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "RequestId")]
    request_id: String,
}

impl S3Error {
    fn status_code(&self) -> StatusCode {
        match self {
            S3Error::NoSuchKey => StatusCode::NOT_FOUND,
            S3Error::NoSuchBucket => StatusCode::NOT_FOUND,
            S3Error::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            S3Error::AccessDenied => StatusCode::FORBIDDEN,
            S3Error::SignatureDoesNotMatch => StatusCode::FORBIDDEN,
            S3Error::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_code(&self) -> &'static str {
        match self {
            S3Error::NoSuchKey => "NoSuchKey",
            S3Error::NoSuchBucket => "NoSuchBucket",
            S3Error::InvalidRequest(_) => "InvalidRequest",
            S3Error::AccessDenied => "AccessDenied",
            S3Error::SignatureDoesNotMatch => "SignatureDoesNotMatch",
            S3Error::InternalError(_) => "InternalError",
        }
    }

    fn message(&self) -> String {
        match self {
            S3Error::NoSuchKey => "The specified key does not exist.".to_string(),
            S3Error::NoSuchBucket => "The specified bucket does not exist.".to_string(),
            S3Error::InvalidRequest(msg) => msg.clone(),
            S3Error::AccessDenied => "Access Denied".to_string(),
            S3Error::SignatureDoesNotMatch => {
                "The request signature we calculated does not match the signature you provided."
                    .to_string()
            }
            S3Error::InternalError(msg) => format!("Internal Error: {}", msg),
        }
    }
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let error_response = S3ErrorResponse {
            code: self.error_code().to_string(),
            message: self.message(),
            request_id: uuid::Uuid::new_v4().to_string(),
        };

        let body = to_xml_string(&error_response).unwrap_or_else(|_| {
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
    <Code>InternalError</Code>
    <Message>Failed to serialize error response</Message>
</Error>"#
                .to_string()
        });

        (
            self.status_code(),
            [("content-type", "application/xml")],
            body,
        )
            .into_response()
    }
}
