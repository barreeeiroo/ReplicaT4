use serde::Serialize;

/// Represents an S3 object metadata
#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: chrono::DateTime<chrono::Utc>,
    pub content_type: String,
}

/// S3 XML response for ListObjectsV2
#[derive(Serialize)]
#[serde(rename = "ListBucketResult")]
pub struct ListBucketResult {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Prefix")]
    pub prefix: Option<String>,
    #[serde(rename = "KeyCount")]
    pub key_count: i32,
    #[serde(rename = "MaxKeys")]
    pub max_keys: i32,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
    #[serde(rename = "Contents")]
    pub contents: Vec<S3Object>,
}

#[derive(Serialize)]
#[serde(rename = "Contents")]
pub struct S3Object {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "LastModified")]
    pub last_modified: String,
    #[serde(rename = "ETag")]
    pub etag: String,
    #[serde(rename = "Size")]
    pub size: u64,
    #[serde(rename = "StorageClass")]
    pub storage_class: String,
}

/// Credentials for SigV4 authentication
#[derive(Debug, Clone)]
pub struct Credentials {
    pub _access_key_id: String,
    pub secret_access_key: String,
}

/// Authentication context passed through request extensions
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub _access_key_id: String,
}