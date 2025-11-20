use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::config::Credentials as AwsCredentials;
use replicat4::{
    AppState, Credentials, CredentialsStore, InMemoryStorage, StorageBackend, create_app,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Test server handle that automatically shuts down on drop
///
/// This starts a real HTTP server on a random port for integration testing.
/// The server uses the actual production code via create_app().
/// It also provides an AWS S3 client configured to communicate with the test server.
pub struct TestServer {
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    #[allow(dead_code)] // Keep handle alive to prevent task abort
    handle: JoinHandle<()>,
    pub client: S3Client,
    pub bucket_name: String,
}

impl TestServer {
    /// Start a test server with in-memory storage and return an S3 client
    pub async fn start(
        bucket_name: String,
        access_key_id: String,
        secret_access_key: String,
    ) -> Self {
        // Create in-memory storage backend
        let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryStorage::new());

        // Create credentials store with test credentials
        let mut credentials_map = HashMap::new();
        credentials_map.insert(
            access_key_id.clone(),
            Credentials {
                _access_key_id: access_key_id.clone(),
                secret_access_key: secret_access_key.clone(),
            },
        );
        let credentials_store = CredentialsStore::new(credentials_map);

        // Create app state
        let app_state = AppState::new(storage, credentials_store, bucket_name.clone());

        // Use the ACTUAL production create_app function
        let app = create_app(app_state, bucket_name.clone());

        // Bind to a random available port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        // Spawn server task
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .unwrap();
        });

        // Give the server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Create AWS S3 client configured to point to our test server
        let endpoint_url = format!("http://{}", addr);
        let creds = AwsCredentials::new(access_key_id, secret_access_key, None, None, "test");

        let config = aws_sdk_s3::config::Builder::new()
            .behavior_version_latest()
            .credentials_provider(creds)
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .endpoint_url(&endpoint_url)
            .force_path_style(true)
            .build();

        let client = S3Client::from_conf(config);

        TestServer {
            shutdown_tx: Some(shutdown_tx),
            handle,
            client,
            bucket_name,
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Signal shutdown (ignore errors if already shut down)
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}
