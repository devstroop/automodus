//! API Server
//!
//! HTTP server setup, routing, and OpenAPI documentation.

use std::sync::Arc;

use axum::http::Method;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::config::AppConfig;

use super::handlers::*;
use super::schemas::*;
use super::state::{create_state, ServerState};

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Automodus API",
        description = "Programmable Workflow Automation Platform API",
        version = "0.1.0",
        contact(name = "Devstroop Technologies", email = "info@devstroop.com"),
        license(name = "MIT")
    ),
    paths(
        health_handler,
        list_workflows_handler,
        reload_workflows_handler,
        get_workflow_handler,
        run_workflow_handler,
        screenshot_handler,
        goto_handler,
        click_handler,
        type_handler,
        wait_handler,
        eval_handler,
        page_info_handler,
    ),
    components(schemas(
        HealthResponse,
        WorkflowInfo,
        WorkflowListResponse,
        ReloadResponse,
        RunWorkflowRequest,
        RunWorkflowResponse,
        GotoRequest,
        GotoResponse,
        ClickRequest,
        TypeRequest,
        WaitRequest,
        EvalRequest,
        BrowserActionResponse,
        PageInfoResponse,
    )),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "workflows", description = "Workflow management and execution"),
        (name = "browser", description = "Browser control endpoints")
    )
)]
pub struct ApiDoc;

/// Create the API router
pub fn create_router(state: Arc<ServerState>) -> Router {
    // CORS configuration
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any)
        .allow_origin(Any);

    Router::new()
        // Health endpoints
        .route("/api/health", get(health_handler))
        // Workflow endpoints
        .route("/api/workflows", get(list_workflows_handler))
        .route("/api/workflows/reload", post(reload_workflows_handler))
        .route("/api/workflows/:name", get(get_workflow_handler))
        .route("/api/workflows/:name/run", post(run_workflow_handler))
        // Browser endpoints
        .route("/api/browser/screenshot", get(screenshot_handler))
        .route("/api/browser/goto", post(goto_handler))
        .route("/api/browser/click", post(click_handler))
        .route("/api/browser/type", post(type_handler))
        .route("/api/browser/wait", post(wait_handler))
        .route("/api/browser/eval", post(eval_handler))
        .route("/api/browser/page", get(page_info_handler))
        // State
        .with_state(state)
        // Swagger UI
        .merge(SwaggerUi::new("/swagger-ui").url("/api/openapi.json", ApiDoc::openapi()))
        // CORS
        .layer(cors)
}

/// Start the API server
///
/// Loads configuration, creates state, loads workflows, and starts serving.
pub async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = AppConfig::load().unwrap_or_else(|e| {
        warn!("Using default config: {}", e);
        AppConfig::default()
    });

    let host = config.server.host.clone();
    let port = config.server.port;

    // Create server state
    let state = create_state(config);

    // Load workflows
    let workflows_dir =
        std::env::var("AUTOMODUS_WORKFLOWS").unwrap_or_else(|_| "workflows".to_string());
    match state.load_workflows(&workflows_dir).await {
        Ok(count) => info!("Loaded {} workflows from {}", count, workflows_dir),
        Err(e) => warn!("Failed to load workflows: {}", e),
    }

    // Create router
    let app = create_router(state);

    // Log startup info
    info!("🚀 Automodus server starting on http://{}:{}", host, port);
    info!("");
    info!("📖 API Documentation:");
    info!("   /swagger-ui              - Swagger UI");
    info!("   /api/openapi.json        - OpenAPI JSON schema");
    info!("");
    info!("📖 API endpoints:");
    info!("   GET  /api/health              - Health check");
    info!("   GET  /api/workflows           - List available workflows");
    info!("   POST /api/workflows/reload    - Reload workflows from disk");
    info!("   GET  /api/workflows/:name     - Get workflow details");
    info!("   POST /api/workflows/:name/run - Execute a workflow");
    info!("   GET  /api/browser/screenshot  - Get browser screenshot");
    info!("   POST /api/browser/goto        - Navigate browser to URL");

    // Bind listener
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port)).await?;

    // Graceful shutdown handler
    let shutdown = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        info!("🛑 Shutting down...");
    };

    // Serve
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}
