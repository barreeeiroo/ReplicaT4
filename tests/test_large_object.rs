mod helpers;

use aws_sdk_s3::primitives::ByteStream;
use helpers::{TEST_ACCESS_KEY_ID, TEST_BUCKET, TEST_SECRET_ACCESS_KEY, TestServer};

#[tokio::test]
async fn test_put_and_get_1mb_object() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "large-file-1mb.bin";
    let test_content = vec![0xAB; 1024 * 1024];

    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from(test_content.clone()))
        .send()
        .await
        .unwrap();

    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    let body = get_result.body.collect().await.unwrap().to_vec();
    assert_eq!(
        body.len(),
        test_content.len(),
        "Retrieved file size should match"
    );
    assert_eq!(
        body.as_slice(),
        test_content.as_slice(),
        "Retrieved content should match"
    );
}

#[tokio::test]
async fn test_put_and_get_5mb_object() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let test_key = "large-file-5mb.bin";
    let test_content = vec![0xCD; 5 * 1024 * 1024];

    server
        .client
        .put_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .body(ByteStream::from(test_content.clone()))
        .send()
        .await
        .unwrap();

    let get_result = server
        .client
        .get_object()
        .bucket(&server.bucket_name)
        .key(test_key)
        .send()
        .await
        .unwrap();

    let body = get_result.body.collect().await.unwrap().to_vec();
    assert_eq!(
        body.len(),
        test_content.len(),
        "Retrieved 5MB file size should match"
    );
    assert_eq!(
        body.as_slice(),
        test_content.as_slice(),
        "Retrieved 5MB content should match"
    );
}

#[tokio::test]
async fn test_multiple_large_objects() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    for i in 0..3 {
        let test_key = format!("multi-large-file-{}.bin", i);
        let test_content = vec![i as u8; 1024 * 1024];

        server
            .client
            .put_object()
            .bucket(&server.bucket_name)
            .key(&test_key)
            .body(ByteStream::from(test_content.clone()))
            .send()
            .await
            .unwrap();

        let get_result = server
            .client
            .get_object()
            .bucket(&server.bucket_name)
            .key(&test_key)
            .send()
            .await
            .unwrap();

        let body = get_result.body.collect().await.unwrap().to_vec();
        assert_eq!(
            body.as_slice(),
            test_content.as_slice(),
            "Content {} should match",
            i
        );
    }
}
