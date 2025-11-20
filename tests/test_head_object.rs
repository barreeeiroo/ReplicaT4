mod helpers;

use aws_sdk_s3::primitives::ByteStream;
use helpers::{TEST_ACCESS_KEY_ID, TEST_BUCKET, TEST_SECRET_ACCESS_KEY, TestServer};

#[tokio::test]
async fn test_head_object_success() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "head-test-file.txt";
    let test_content = b"Content for HEAD test";

    // PUT an object
    let put_result = server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(test_content))
        .send()
        .await
        .unwrap();

    let put_etag = put_result.e_tag.unwrap();

    // HEAD the object
    let head_result = server
        .client
        .head_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    // Verify metadata
    assert!(head_result.e_tag.is_some(), "ETag should be present");
    assert!(
        head_result.content_type.is_some(),
        "Content-Type should be present"
    );
    assert!(
        head_result.last_modified.is_some(),
        "Last-Modified should be present"
    );
    assert_eq!(
        head_result.content_length(),
        Some(test_content.len() as i64),
        "Content-Length should match"
    );

    let head_etag = head_result.e_tag.unwrap();
    assert_eq!(
        head_etag, put_etag,
        "ETag from HEAD should match ETag from PUT"
    );
}

#[tokio::test]
async fn test_head_object_not_found() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    // HEAD a non-existent object
    let head_result = server
        .client
        .head_object()
        .bucket(&server.bucket_name)
        .key("nonexistent-file.txt")
        .send()
        .await;

    // Should return error (NotFound)
    assert!(
        head_result.is_err(),
        "HEAD should return error for non-existent object"
    );
}

#[tokio::test]
async fn test_head_object_empty_file() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "empty-head-file.txt";
    let test_content = b"";

    // PUT an empty object
    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(test_content))
        .send()
        .await
        .unwrap();

    // HEAD the empty object
    let head_result = server
        .client
        .head_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    assert_eq!(
        head_result.content_length(),
        Some(0),
        "Empty file should have content-length of 0"
    );
}

#[tokio::test]
async fn test_head_vs_get_consistency() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "consistency-test.txt";
    let test_content = b"Testing HEAD vs GET consistency";

    // PUT an object
    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(test_content))
        .send()
        .await
        .unwrap();

    // HEAD the object
    let head_result = server
        .client
        .head_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    let head_etag = head_result.e_tag.as_ref().unwrap();
    let head_content_length = head_result.content_length();

    // GET the object
    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    let get_etag = get_result.e_tag.as_ref().unwrap();
    let get_content_length = get_result.content_length();

    // Verify HEAD and GET return the same metadata
    assert_eq!(head_etag, get_etag, "HEAD and GET should return same ETag");
    assert_eq!(
        head_content_length, get_content_length,
        "HEAD and GET should return same Content-Length"
    );
}
