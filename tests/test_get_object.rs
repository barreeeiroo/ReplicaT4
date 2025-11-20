mod helpers;

use aws_sdk_s3::primitives::ByteStream;
use helpers::{TEST_ACCESS_KEY_ID, TEST_BUCKET, TEST_SECRET_ACCESS_KEY, TestServer};

#[tokio::test]
async fn test_get_object_success() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "test-file-get.txt";
    let test_content = b"Hello, World from GET test!";

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

    // GET the object
    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    // Verify metadata
    assert!(get_result.e_tag.is_some(), "ETag should be present");
    assert!(
        get_result.content_type.is_some(),
        "Content-Type should be present"
    );
    assert!(
        get_result.last_modified.is_some(),
        "Last-Modified should be present"
    );
    assert_eq!(
        get_result.content_length(),
        Some(test_content.len() as i64),
        "Content-Length should match"
    );

    // Verify body
    let body = get_result.body.collect().await.unwrap().to_vec();
    assert_eq!(
        body.as_slice(),
        test_content,
        "Response body should match uploaded content"
    );
}

#[tokio::test]
async fn test_get_object_not_found() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    // Try to GET a non-existent object
    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key("nonexistent-file.txt")
        .send()
        .await;

    // Should return error (NoSuchKey)
    assert!(
        get_result.is_err(),
        "Non-existent object should return error"
    );
}

#[tokio::test]
async fn test_get_object_empty_content() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "empty-file.txt";
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

    // GET the empty object
    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    assert_eq!(
        get_result.content_length(),
        Some(0),
        "Empty file should have content-length of 0"
    );

    let body = get_result.body.collect().await.unwrap().to_vec();
    assert_eq!(body.len(), 0, "Empty file should return empty body");
}

#[tokio::test]
async fn test_get_object_with_special_characters_in_key() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "path/to/file-with-special_chars.txt";
    let test_content = b"Content with special key";

    // PUT an object with special characters in key
    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(test_content))
        .send()
        .await
        .unwrap();

    // GET the object
    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    let body = get_result.body.collect().await.unwrap().to_vec();
    assert_eq!(body.as_slice(), test_content);
}
