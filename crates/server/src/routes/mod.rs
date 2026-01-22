use axum::{
    Router,
    middleware as axum_middleware,
    routing::{IntoMakeService, get},
};
use db::DeploymentMode;
use tower_http::validate_request::ValidateRequestHeaderLayer;

use crate::{DeploymentImpl, middleware};

pub mod approvals;
pub mod config;
pub mod containers;
pub mod filesystem;
// pub mod github;
pub mod events;
pub mod execution_processes;
pub mod frontend;
pub mod health;
pub mod images;
pub mod oauth;
pub mod organizations;
pub mod projects;
pub mod repo;
pub mod scratch;
pub mod sessions;
pub mod tags;
pub mod task_attempts;
pub mod tasks;
pub mod terminal;

pub fn router(deployment: DeploymentImpl) -> IntoMakeService<Router> {
    let mode = DeploymentMode::detect();

    // Routes that require authentication in K8s mode
    let protected_routes = Router::new()
        .merge(config::router())
        .merge(containers::router(&deployment))
        .merge(projects::router(&deployment))
        .merge(tasks::router(&deployment))
        .merge(task_attempts::router(&deployment))
        .merge(execution_processes::router(&deployment))
        .merge(tags::router(&deployment))
        .merge(oauth::router())
        .merge(organizations::router())
        .merge(filesystem::router())
        .merge(repo::router())
        .merge(events::router(&deployment))
        .merge(approvals::router())
        .merge(scratch::router(&deployment))
        .merge(sessions::router(&deployment))
        .merge(terminal::router())
        .nest("/images", images::routes());

    // Apply auth middleware conditionally based on deployment mode
    let protected_routes = if mode.is_kubernetes() {
        tracing::info!(
            mode = "kubernetes",
            "Applying authentication middleware to protected routes"
        );
        protected_routes.layer(axum_middleware::from_fn(middleware::require_user))
    } else {
        tracing::info!(
            mode = "desktop",
            "Skipping authentication middleware for desktop mode"
        );
        protected_routes
    };

    // Health check is always public (unprotected)
    let base_routes = Router::new()
        .route("/health", get(health::health_check))
        .merge(protected_routes)
        .layer(ValidateRequestHeaderLayer::custom(
            middleware::validate_origin,
        ))
        .with_state(deployment);

    Router::new()
        .route("/", get(frontend::serve_frontend_root))
        .route("/{*path}", get(frontend::serve_frontend))
        .nest("/api", base_routes)
        .into_make_service()
}
