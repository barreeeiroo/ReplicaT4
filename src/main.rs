mod app_state;
mod auth;
mod config;
mod handlers;
mod storage;
mod types;

use app_state::AppState;
use auth::{CredentialsStore, auth_middleware};
use config::{BackendConfig, Config};
use handlers::{delete_object, get_object, head_bucket, head_object, list_objects, not_found, put_object};
use storage::{InMemoryStorage, S3Backend, StorageBackend};
use types::Credentials;

use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    routing::get,
};
use clap::Parser;
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

// Server configuration
const HOST: &str = "0.0.0.0";
const PORT: u16 = 3000;

// Default configuration values
const DEFAULT_ACCESS_KEY_ID: &str = "AKIAIOSFODNN7EXAMPLE";
const DEFAULT_SECRET_ACCESS_KEY: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const DEFAULT_BUCKET_NAME: &str = "mybucket";

/// ReplicaT4: S3-compatible proxy server with multi-backend replication
#[derive(Parser, Debug)]
#[command(name = "replicat4")]
#[command(about = "Proxy Service to Replicate Data into Multiple S3 Compatible Destinations", long_about = None)]
struct Cli {
    /// Path to the configuration file (required)
    #[arg(short, long, env = "CONFIG_PATH")]
    config: String,

    /// Host to bind to
    #[arg(long, env = "HOST", default_value = HOST)]
    host: String,

    /// Port to listen on
    #[arg(short, long, env = "PORT", default_value_t = PORT)]
    port: u16,

    /// AWS Access Key ID for incoming requests
    #[arg(long, env = "AWS_ACCESS_KEY_ID", default_value = DEFAULT_ACCESS_KEY_ID)]
    access_key_id: String,

    /// AWS Secret Access Key for incoming requests
    #[arg(long, env = "AWS_SECRET_ACCESS_KEY", default_value = DEFAULT_SECRET_ACCESS_KEY, hide_env_values = true)]
    secret_access_key: String,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let cli = Cli::parse();

    // Load backend configuration from file
    let config = match Config::from_file(&cli.config) {
        Ok(cfg) => {
            tracing::info!("Loaded configuration from {}", cli.config);
            cfg
        }
        Err(e) => {
            tracing::error!("Failed to load config file '{}': {}", cli.config, e);
            tracing::error!(
                "Configuration file is required. Use --config <path> or set CONFIG_PATH environment variable."
            );
            std::process::exit(1);
        }
    };

    // Determine bucket name: config file > default
    let bucket_name = config
        .virtual_bucket
        .clone()
        .unwrap_or_else(|| DEFAULT_BUCKET_NAME.to_string());

    tracing::info!("Using bucket: {}", bucket_name);
    tracing::info!("Using access key: {}", cli.access_key_id);

    // Initialize backends from configuration
    let mut backends: Vec<Arc<dyn StorageBackend>> = Vec::new();

    for backend_config in config.backends {
        match backend_config {
            BackendConfig::S3(s3_config) => {
                tracing::info!("Initializing S3 backend: {}", s3_config.name);
                match S3Backend::new(
                    s3_config.name.clone(),
                    s3_config.bucket,
                    s3_config.region,
                    s3_config.endpoint,
                    s3_config.force_path_style,
                    s3_config.access_key_id,
                    s3_config.secret_access_key,
                )
                .await
                {
                    Ok(backend) => {
                        tracing::info!(
                            "✓ S3 backend '{}' initialized successfully",
                            s3_config.name
                        );
                        backends.push(Arc::new(backend));
                    }
                    Err(e) => {
                        tracing::error!(
                            "✗ Failed to initialize S3 backend '{}': {}",
                            s3_config.name,
                            e
                        );
                    }
                }
            }
            BackendConfig::Memory(mem_config) => {
                tracing::info!("Initializing in-memory backend: {}", mem_config.name);
                backends.push(Arc::new(InMemoryStorage::new()));
                tracing::info!("✓ In-memory backend '{}' initialized", mem_config.name);
            }
        }
    }

    if backends.is_empty() {
        tracing::error!("No backends configured! Exiting.");
        std::process::exit(1);
    }

    // Create storage backend (with replication if multiple backends)
    let storage: Arc<dyn StorageBackend> = if backends.len() == 1 {
        tracing::info!("Using single backend (no replication)");
        backends.into_iter().next().unwrap()
    } else {
        tracing::info!("Using single backend (no replication)");
        // tracing::info!("Using multi-backend replication with {} backends", backends.len());
        // Arc::new(MultiBackend::new(backends))
        backends.into_iter().next().unwrap()
    };

    // Create credentials store
    let mut credentials_map = HashMap::new();
    credentials_map.insert(
        cli.access_key_id.clone(),
        Credentials {
            _access_key_id: cli.access_key_id.clone(),
            secret_access_key: cli.secret_access_key.clone(),
        },
    );
    let credentials_store = CredentialsStore::new(credentials_map);

    // Create shared app state
    let app_state = AppState::new(storage, credentials_store, bucket_name.clone());

    // Build router with S3 API endpoints using the bucket name as a constant path
    let bucket_path = format!("/{}", bucket_name);
    let bucket_path_with_slash = format!("/{}/", bucket_name);
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
        // Bucket operations: /{bucket_name} and /{bucket_name}/
        .route(&bucket_path, get(list_objects).head(head_bucket))
        .route(&bucket_path_with_slash, get(list_objects).head(head_bucket))
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
    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    tracing::info!(
        "S3-compatible API server listening on {}",
        listener.local_addr().unwrap()
    );
    tracing::info!("Configured bucket: {}", bucket_name);
    tracing::info!(
        "Example: aws s3 --endpoint-url http://localhost:{} ls s3://{}/",
        cli.port,
        bucket_name
    );

    axum::serve(listener, app).await.unwrap();
}
