mod helpers;

use aws_sdk_s3::primitives::ByteStream;
use helpers::{TEST_ACCESS_KEY_ID, TEST_BUCKET, TEST_SECRET_ACCESS_KEY, TestServer};

#[tokio::test]
async fn test_delete_object_success() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "file-to-delete.txt";
    let test_content = b"This will be deleted";

    // PUT an object using AWS SDK
    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(test_content))
        .send()
        .await
        .unwrap();

    // Verify object exists with GET
    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await;
    assert!(get_result.is_ok(), "Object should exist before deletion");

    // DELETE the object
    server
        .client
        .delete_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    // Verify object no longer exists
    let get_result2 = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await;
    assert!(
        get_result2.is_err(),
        "Object should not exist after deletion"
    );
}

#[tokio::test]
async fn test_delete_nonexistent_object() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    // DELETE a non-existent object (should be idempotent)
    let result = server
        .client
        .delete_object()
        .bucket(&server.bucket_name)
        .key("nonexistent-file.txt")
        .send()
        .await;

    // S3 DELETE is idempotent - should succeed even if object doesn't exist
    assert!(result.is_ok(), "DELETE should be idempotent");
}

#[tokio::test]
async fn test_delete_multiple_objects() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    // PUT multiple objects
    for i in 0..3 {
        let test_key = format!("file-{}.txt", i);
        let test_content = format!("Content {}", i);

        server
            .client
            .put_object()
            .bucket(&server.bucket_name)
            .key(&test_key)
            .body(ByteStream::from(test_content.into_bytes()))
            .send()
            .await
            .unwrap();
    }

    // DELETE each object
    for i in 0..3 {
        let test_key = format!("file-{}.txt", i);

        server
            .client
            .delete_object()
            .bucket(&server.bucket_name)
            .key(&test_key)
            .send()
            .await
            .unwrap();

        // Verify deletion
        let get_result = server
            .client
            .get_object()
            .bucket(&server.bucket_name)
            .key(&test_key)
            .send()
            .await;
        assert!(get_result.is_err(), "Object {} should be deleted", i);
    }
}

#[tokio::test]
async fn test_delete_and_recreate() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "recreate-file.txt";
    let content1 = b"First version";
    let content2 = b"Second version after deletion";

    // PUT first version
    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(content1))
        .send()
        .await
        .unwrap();

    // DELETE
    server
        .client
        .delete_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    // PUT second version
    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from_static(content2))
        .send()
        .await
        .unwrap();

    // GET and verify it's the second version
    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    let body = get_result.body.collect().await.unwrap().to_vec();
    assert_eq!(body.as_slice(), content2, "Should retrieve second version");
}
