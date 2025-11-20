mod helpers;

use aws_sdk_s3::primitives::ByteStream;
use helpers::{TEST_ACCESS_KEY_ID, TEST_BUCKET, TEST_SECRET_ACCESS_KEY, TestServer};

#[tokio::test]
async fn test_head_bucket_success() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let head_result = server
        .client
        .head_bucket()
        .bucket(&server.bucket_name)
        .send()
        .await;

    assert!(head_result.is_ok(), "HEAD bucket should return success");
}

#[tokio::test]
async fn test_head_bucket_empty() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let head_result = server
        .client
        .head_bucket()
        .bucket(&server.bucket_name)
        .send()
        .await;

    assert!(
        head_result.is_ok(),
        "HEAD on empty bucket should return success"
    );
}

#[tokio::test]
async fn test_head_bucket_with_objects() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    for i in 0..3 {
        let test_key = format!("file-{}.txt", i);
        let test_content = format!("Content {}", i);

        server
            .client
            .put_object()
            .bucket(&server.bucket_name)
            .key(test_key)
            .body(ByteStream::from(test_content.into_bytes()))
            .send()
            .await
            .unwrap();
    }

    let head_result = server
        .client
        .head_bucket()
        .bucket(&server.bucket_name)
        .send()
        .await;

    assert!(
        head_result.is_ok(),
        "HEAD bucket with objects should return success"
    );
}
