use crate::{
    app_state::AppState,
    types::{error::S3Error, AuthContext, ListBucketResult, S3Object},
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension,
};
use quick_xml::se::to_string as to_xml_string;
use serde::Deserialize;

/// Query parameters for ListObjectsV2
#[derive(Deserialize)]
pub struct ListObjectsQuery {
    #[serde(rename = "list-type")]
    _list_type: Option<String>,
    prefix: Option<String>,
    #[serde(rename = "max-keys")]
    max_keys: Option<i32>,
}

/// GET /{bucket_name}?list-type=2 - List objects in a bucket
pub async fn list_objects(
    Query(params): Query<ListObjectsQuery>,
    State(app_state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, S3Error> {
    let storage = &app_state.storage;
    let bucket = &app_state.bucket_name;
    tracing::info!("LIST objects: bucket={}, prefix={:?}", bucket, params.prefix);

    let max_keys = params.max_keys.unwrap_or(1000).min(1000);
    let prefix = params.prefix.as_deref();

    // Get objects from storage
    let objects = storage.list_objects(prefix, max_keys).await?;

    // Convert to S3 XML format
    let s3_objects: Vec<S3Object> = objects
        .iter()
        .map(|obj| S3Object {
            key: obj.key.clone(),
            last_modified: obj.last_modified.to_rfc3339(),
            etag: obj.etag.clone(),
            size: obj.size,
            storage_class: "STANDARD".to_string(),
        })
        .collect();

    let response = ListBucketResult {
        name: bucket.to_string(),
        prefix: params.prefix,
        key_count: s3_objects.len() as i32,
        max_keys,
        is_truncated: false,
        contents: s3_objects,
    };

    // Serialize to XML
    let xml = to_xml_string(&response)
        .map_err(|e| S3Error::InternalError(format!("Failed to serialize XML: {}", e)))?;

    let xml_with_header = format!(r#"<?xml version="1.0" encoding="UTF-8"?>{}"#, xml);

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        xml_with_header,
    ))
}
