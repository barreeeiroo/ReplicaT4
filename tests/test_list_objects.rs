mod helpers;

use aws_sdk_s3::primitives::ByteStream;
use helpers::{TEST_ACCESS_KEY_ID, TEST_BUCKET, TEST_SECRET_ACCESS_KEY, TestServer};

#[tokio::test]
async fn test_list_objects_empty_bucket() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let list_result = server
        .client
        .list_objects_v2()
        .bucket(&server.bucket_name)
        .send()
        .await
        .unwrap();

    assert_eq!(
        list_result.key_count(),
        Some(0),
        "Empty bucket should have 0 keys"
    );
}

#[tokio::test]
async fn test_list_objects_with_objects() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let keys = vec!["file1.txt", "file2.txt", "file3.txt"];
    for key in &keys {
        let test_content = format!("Content of {}", key);
        server
            .client
            .put_object()
            .bucket(&server.bucket_name)
            .key(*key)
            .body(ByteStream::from(test_content.into_bytes()))
            .send()
            .await
            .unwrap();
    }

    let list_result = server
        .client
        .list_objects_v2()
        .bucket(&server.bucket_name)
        .send()
        .await
        .unwrap();

    assert_eq!(list_result.key_count(), Some(3), "Should list 3 objects");

    let listed_keys: Vec<String> = list_result
        .contents()
        .iter()
        .map(|obj| obj.key().unwrap().to_string())
        .collect();

    for key in &keys {
        assert!(
            listed_keys.contains(&key.to_string()),
            "Should contain key: {}",
            key
        );
    }
}

#[tokio::test]
async fn test_list_objects_with_prefix() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    let objects = vec![
        "documents/file1.txt",
        "documents/file2.txt",
        "images/photo1.jpg",
        "readme.txt",
    ];

    for key in &objects {
        let test_content = format!("Content of {}", key);
        server
            .client
            .put_object()
            .bucket(&server.bucket_name)
            .key(*key)
            .body(ByteStream::from(test_content.into_bytes()))
            .send()
            .await
            .unwrap();
    }

    let list_result = server
        .client
        .list_objects_v2()
        .bucket(&server.bucket_name)
        .prefix("documents/")
        .send()
        .await
        .unwrap();

    assert_eq!(
        list_result.key_count(),
        Some(2),
        "Should list 2 objects with prefix documents/"
    );
}

#[tokio::test]
async fn test_list_objects_with_max_keys() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    for i in 0..10 {
        let key = format!("file-{:02}.txt", i);
        let test_content = format!("Content {}", i);
        server
            .client
            .put_object()
            .bucket(&server.bucket_name)
            .key(&key)
            .body(ByteStream::from(test_content.into_bytes()))
            .send()
            .await
            .unwrap();
    }

    let list_result = server
        .client
        .list_objects_v2()
        .bucket(&server.bucket_name)
        .max_keys(5)
        .send()
        .await
        .unwrap();

    assert_eq!(
        list_result.key_count(),
        Some(5),
        "Should return exactly 5 keys when max-keys=5"
    );
}

#[tokio::test]
async fn test_list_objects_after_deletion() {
    let server = TestServer::start(
        TEST_BUCKET.to_string(),
        TEST_ACCESS_KEY_ID.to_string(),
        TEST_SECRET_ACCESS_KEY.to_string(),
    )
    .await;

    for i in 0..3 {
        let key = format!("file-{}.txt", i);
        let test_content = format!("Content {}", i);
        server
            .client
            .put_object()
            .bucket(&server.bucket_name)
            .key(&key)
            .body(ByteStream::from(test_content.into_bytes()))
            .send()
            .await
            .unwrap();
    }

    server
        .client
        .delete_object()
        .bucket(&server.bucket_name)
        .key("file-1.txt")
        .send()
        .await
        .unwrap();

    let list_result = server
        .client
        .list_objects_v2()
        .bucket(&server.bucket_name)
        .send()
        .await
        .unwrap();

    assert_eq!(
        list_result.key_count(),
        Some(2),
        "Should list 2 objects after deletion"
    );

    let listed_keys: Vec<String> = list_result
        .contents()
        .iter()
        .map(|obj| obj.key().unwrap().to_string())
        .collect();

    assert!(listed_keys.contains(&"file-0.txt".to_string()));
    assert!(listed_keys.contains(&"file-2.txt".to_string()));
    assert!(
        !listed_keys.contains(&"file-1.txt".to_string()),
        "Should not list deleted file"
    );
}
