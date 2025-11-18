mod app_state;
mod auth;
mod handlers;
mod storage;
mod types;

use app_state::AppState;
use auth::{CredentialsStore, auth_middleware};
use handlers::{delete_object, get_object, head_object, list_objects, not_found, put_object};
use storage::{InMemoryStorage, StorageBackend};
use types::Credentials;

use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    routing::get,
};
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

// Server configuration
const HOST: &str = "127.0.0.1";
const PORT: u16 = 3000;

// Default configuration values
const DEFAULT_ACCESS_KEY_ID: &str = "AKIAIOSFODNN7EXAMPLE";
const DEFAULT_SECRET_ACCESS_KEY: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const DEFAULT_BUCKET_NAME: &str = "mybucket";

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load configuration from environment variables or use defaults
    let access_key_id =
        std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| DEFAULT_ACCESS_KEY_ID.to_string());
    let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY")
        .unwrap_or_else(|_| DEFAULT_SECRET_ACCESS_KEY.to_string());
    let bucket_name =
        std::env::var("BUCKET_NAME").unwrap_or_else(|_| DEFAULT_BUCKET_NAME.to_string());

    tracing::info!("Using bucket: {}", bucket_name);
    tracing::info!("Using access key: {}", access_key_id);

    // Create storage backend
    let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryStorage::new());

    // Create credentials store
    let mut credentials_map = HashMap::new();
    credentials_map.insert(
        access_key_id.clone(),
        Credentials {
            _access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
        },
    );
    let credentials_store = CredentialsStore::new(credentials_map);

    // Create shared app state
    let app_state = AppState::new(storage, credentials_store, bucket_name.clone());

    // Build router with S3 API endpoints using the bucket name as a constant path
    let bucket_path = format!("/{}", bucket_name);
    let object_path = format!("/{}/{{*key}}", bucket_name);

    let app = Router::new()
        // Object operations: /{bucket_name}/{key}
        .route(
            &object_path,
            get(get_object)
                .put(put_object)
                .delete(delete_object)
                .head(head_object),
        )
        // List objects: /{bucket_name}
        .route(&bucket_path, get(list_objects))
        // Fallback for 404 Not Found
        .fallback(not_found)
        // Add shared state
        .with_state(app_state.clone())
        // Add authentication middleware (captures app_state)
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            let state = app_state.clone();
            async move { auth_middleware(state, request, next).await }
        }))
        // Add tracing
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = format!("{}:{}", HOST, PORT);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    tracing::info!(
        "S3-compatible API server listening on {}",
        listener.local_addr().unwrap()
    );
    tracing::info!("Configured bucket: {}", bucket_name);
    tracing::info!(
        "Example: aws s3 --endpoint-url http://{}:{} ls s3://{}/",
        HOST,
        PORT,
        bucket_name
    );

    axum::serve(listener, app).await.unwrap();
}
