mod helpers;

use aws_sdk_s3::primitives::ByteStream;
use helpers::{TEST_ACCESS_KEY_ID, TEST_BUCKET, TEST_SECRET_ACCESS_KEY, TestServer};

#[tokio::test]
async fn test_put_object_success() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "test-file.txt";
    let test_content = b"Hello, World!";

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

    assert!(put_result.e_tag.is_some(), "ETag header should be present");
}

#[tokio::test]
async fn test_put_object_empty_content() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "empty-file.txt";
    let test_content = b"";

    // PUT an empty object
    let put_result = server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(test_content))
        .send()
        .await
        .unwrap();

    assert!(put_result.e_tag.is_some());
}

#[tokio::test]
async fn test_put_object_overwrite() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "overwrite-file.txt";

    // First PUT
    let content1 = b"First version";
    let put_result1 = server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(content1))
        .send()
        .await
        .unwrap();

    let etag1 = put_result1.e_tag.unwrap();

    // Second PUT (overwrite)
    let content2 = b"Second version - different content";
    let put_result2 = server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(content2))
        .send()
        .await
        .unwrap();

    let etag2 = put_result2.e_tag.unwrap();

    // ETags should be different since content changed
    assert_ne!(etag1, etag2, "ETags should differ for different content");
}
