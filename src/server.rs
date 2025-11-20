use crate::{app_state::AppState, auth, handlers};
use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    routing::get,
};
use tower_http::trace::TraceLayer;

/// Create the application router with all routes and middleware
///
/// This function is used by both main.rs and integration tests to ensure
/// the same server configuration is used in both production and tests.
pub fn create_app(app_state: AppState, bucket_name: String) -> Router {
    use handlers::{
        delete_object, get_object, head_bucket, head_object, list_objects, not_found, put_object,
    };

    let bucket_path = format!("/{}", bucket_name);
    let bucket_path_with_slash = format!("/{}/", bucket_name);
    let object_path = format!("/{}/{{*key}}", bucket_name);

    Router::new()
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
            async move { auth::auth_middleware(state, request, next).await }
        }))
        // Add tracing
        .layer(TraceLayer::new_for_http())
}
