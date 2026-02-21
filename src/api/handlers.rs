//! API Handlers
//!
//! Request handlers for the REST API endpoints.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use tracing::info;

use crate::actions::BrowserHandle;

use super::schemas::*;
use super::state::ServerState;

/// Version constant for health check
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// Health
// ============================================================================

/// Health check endpoint
///
/// Returns server status, version, and basic metrics.
#[utoipa::path(
    get,
    path = "/api/health",
    tag = "health",
    responses(
        (status = 200, description = "Server is healthy", body = HealthResponse)
    )
)]
pub async fn health_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let workflows = state.workflows.read().await;
    let browser = state.browser.lock().await;

    Json(HealthResponse {
        status: "ok".to_string(),
        version: VERSION.to_string(),
        workflows_loaded: workflows.len(),
        browser_running: browser.is_some(),
    })
}

// ============================================================================
// Workflows
// ============================================================================

/// List all available workflows
#[utoipa::path(
    get,
    path = "/api/workflows",
    tag = "workflows",
    responses(
        (status = 200, description = "List of available workflows", body = WorkflowListResponse)
    )
)]
pub async fn list_workflows_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let workflows = state.workflows.read().await;

    let workflow_list: Vec<WorkflowInfo> = workflows
        .values()
        .map(|w| WorkflowInfo {
            name: w.name.clone(),
            description: w.description.clone(),
            steps: w.steps.len(),
            params: w.params.keys().cloned().collect(),
        })
        .collect();

    Json(WorkflowListResponse {
        workflows: workflow_list,
    })
}

/// Reload workflows from disk
#[utoipa::path(
    post,
    path = "/api/workflows/reload",
    tag = "workflows",
    responses(
        (status = 200, description = "Workflows reloaded", body = ReloadResponse)
    )
)]
pub async fn reload_workflows_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let workflows_dir =
        std::env::var("AUTOMODUS_WORKFLOWS").unwrap_or_else(|_| "workflows".to_string());

    match state.load_workflows(&workflows_dir).await {
        Ok(count) => Json(ReloadResponse {
            success: true,
            count,
            message: format!("Loaded {} workflows", count),
        }),
        Err(e) => Json(ReloadResponse {
            success: false,
            count: 0,
            message: e.to_string(),
        }),
    }
}

/// Get workflow details by name
#[utoipa::path(
    get,
    path = "/api/workflows/{name}",
    tag = "workflows",
    params(
        ("name" = String, Path, description = "Workflow name")
    ),
    responses(
        (status = 200, description = "Workflow details", body = WorkflowInfo),
        (status = 404, description = "Workflow not found")
    )
)]
pub async fn get_workflow_handler(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let workflows = state.workflows.read().await;

    if let Some(workflow) = workflows.get(&name) {
        let info = WorkflowInfo {
            name: workflow.name.clone(),
            description: workflow.description.clone(),
            steps: workflow.steps.len(),
            params: workflow.params.keys().cloned().collect(),
        };
        (StatusCode::OK, Json(serde_json::json!(info)))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("Workflow '{}' not found", name)
            })),
        )
    }
}

/// Execute a workflow by name
#[utoipa::path(
    post,
    path = "/api/workflows/{name}/run",
    tag = "workflows",
    params(
        ("name" = String, Path, description = "Workflow name to execute")
    ),
    request_body = RunWorkflowRequest,
    responses(
        (status = 200, description = "Workflow execution result", body = RunWorkflowResponse),
        (status = 404, description = "Workflow not found"),
        (status = 500, description = "Execution error")
    )
)]
pub async fn run_workflow_handler(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
    Json(request): Json<RunWorkflowRequest>,
) -> impl IntoResponse {
    // Get workflow
    let workflow = {
        let workflows = state.workflows.read().await;
        workflows.get(&name).cloned()
    };

    let Some(workflow) = workflow else {
        return (
            StatusCode::NOT_FOUND,
            Json(RunWorkflowResponse {
                success: false,
                workflow_name: name,
                duration_ms: 0,
                steps_executed: 0,
                output: serde_json::Value::Null,
                error: Some("Workflow not found".to_string()),
            }),
        );
    };

    // Get browser page
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RunWorkflowResponse {
                    success: false,
                    workflow_name: name,
                    duration_ms: 0,
                    steps_executed: 0,
                    output: serde_json::Value::Null,
                    error: Some(format!("Browser error: {}", e)),
                }),
            );
        }
    };

    // Execute workflow
    info!("Executing workflow: {}", workflow.name);

    let result = state
        .engine
        .execute(&workflow, &adapter, request.params)
        .await;

    match result {
        Ok(result) => (
            StatusCode::OK,
            Json(RunWorkflowResponse {
                success: result.success,
                workflow_name: result.workflow_name,
                duration_ms: result.duration_ms,
                steps_executed: result.steps_executed,
                output: result.output,
                error: result.error,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RunWorkflowResponse {
                success: false,
                workflow_name: name,
                duration_ms: 0,
                steps_executed: 0,
                output: serde_json::Value::Null,
                error: Some(e.to_string()),
            }),
        ),
    }
}

// ============================================================================
// Browser
// ============================================================================

/// Get browser screenshot
#[utoipa::path(
    get,
    path = "/api/browser/screenshot",
    tag = "browser",
    responses(
        (status = 200, description = "Screenshot as PNG image", content_type = "image/png"),
        (status = 500, description = "Browser error")
    )
)]
pub async fn screenshot_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                format!(r#"{{"error":"{}"}}"#, e).into_bytes(),
            );
        }
    };

    match adapter.screenshot(false).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "image/png")],
            bytes,
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            format!(r#"{{"error":"{}"}}"#, e).into_bytes(),
        ),
    }
}

/// Navigate browser to URL
#[utoipa::path(
    post,
    path = "/api/browser/goto",
    tag = "browser",
    request_body = GotoRequest,
    responses(
        (status = 200, description = "Navigation result", body = GotoResponse)
    )
)]
pub async fn goto_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<GotoRequest>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(GotoResponse {
                success: false,
                url: request.url,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    match adapter.goto(&request.url).await {
        Ok(()) => {
            let current_url = adapter.current_url().await.unwrap_or_default();
            Json(GotoResponse {
                success: true,
                url: current_url,
                error: None,
            })
        }
        Err(e) => Json(GotoResponse {
            success: false,
            url: request.url,
            error: Some(e.to_string()),
        }),
    }
}
