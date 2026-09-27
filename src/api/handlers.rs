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
use super::state::{ServerState, WebSocketPauseHandler};

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

    Json(HealthResponse {
        status: "ok".to_string(),
        version: VERSION.to_string(),
        workflows_loaded: workflows.len(),
        browser_running: state.core.has_browser().await,
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

    // Execute workflow with tracking
    info!("Executing workflow: {}", workflow.name);

    let total_steps = workflow.steps.len();
    let (exec_id, cancel_token) = state
        .start_execution(&workflow.name, total_steps, request.params.clone())
        .await;

    let pause_handler = WebSocketPauseHandler::new(exec_id.clone(), state.clone());

    let result = state
        .engine
        .execute_with_pause_handler(
            &workflow,
            &adapter,
            request.params,
            crate::workflow::schema::ResolvedDebugConfig::default(),
            &pause_handler,
            Some(cancel_token),
        )
        .await;

    match result {
        Ok(result) => {
            state
                .complete_execution(
                    &exec_id,
                    result.success,
                    result.output.clone(),
                    result.error.clone(),
                )
                .await;
            (
                StatusCode::OK,
                Json(RunWorkflowResponse {
                    success: result.success,
                    workflow_name: result.workflow_name,
                    duration_ms: result.duration_ms,
                    steps_executed: result.steps_executed,
                    output: result.output,
                    error: result.error,
                }),
            )
        }
        Err(e) => {
            state
                .complete_execution(
                    &exec_id,
                    false,
                    serde_json::Value::Null,
                    Some(e.to_string()),
                )
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RunWorkflowResponse {
                    success: false,
                    workflow_name: name,
                    duration_ms: 0,
                    steps_executed: 0,
                    output: serde_json::Value::Null,
                    error: Some(e.to_string()),
                }),
            )
        }
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

// ============================================================================
// Browser Control
// ============================================================================

/// Click an element
#[utoipa::path(
    post,
    path = "/api/browser/click",
    tag = "browser",
    request_body = ClickRequest,
    responses(
        (status = 200, description = "Click result", body = BrowserActionResponse)
    )
)]
pub async fn click_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<ClickRequest>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(BrowserActionResponse {
                success: false,
                result: None,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    match adapter.click(&request.selector).await {
        Ok(()) => Json(BrowserActionResponse {
            success: true,
            result: None,
            error: None,
        }),
        Err(e) => Json(BrowserActionResponse {
            success: false,
            result: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Type into an element
#[utoipa::path(
    post,
    path = "/api/browser/type",
    tag = "browser",
    request_body = TypeRequest,
    responses(
        (status = 200, description = "Type result", body = BrowserActionResponse)
    )
)]
pub async fn type_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<TypeRequest>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(BrowserActionResponse {
                success: false,
                result: None,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    // clear: false by default - append text
    match adapter
        .type_text(&request.selector, &request.text, false)
        .await
    {
        Ok(()) => Json(BrowserActionResponse {
            success: true,
            result: None,
            error: None,
        }),
        Err(e) => Json(BrowserActionResponse {
            success: false,
            result: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Wait for an element
#[utoipa::path(
    post,
    path = "/api/browser/wait",
    tag = "browser",
    request_body = WaitRequest,
    responses(
        (status = 200, description = "Wait result", body = BrowserActionResponse)
    )
)]
pub async fn wait_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<WaitRequest>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(BrowserActionResponse {
                success: false,
                result: None,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    match adapter.wait_for(&request.selector, request.timeout).await {
        Ok(()) => Json(BrowserActionResponse {
            success: true,
            result: None,
            error: None,
        }),
        Err(e) => Json(BrowserActionResponse {
            success: false,
            result: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Execute JavaScript
#[utoipa::path(
    post,
    path = "/api/browser/eval",
    tag = "browser",
    request_body = EvalRequest,
    responses(
        (status = 200, description = "Eval result", body = BrowserActionResponse)
    )
)]
pub async fn eval_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<EvalRequest>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(BrowserActionResponse {
                success: false,
                result: None,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    match adapter.eval(&request.script).await {
        Ok(value) => Json(BrowserActionResponse {
            success: true,
            result: Some(value),
            error: None,
        }),
        Err(e) => Json(BrowserActionResponse {
            success: false,
            result: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Get page info
#[utoipa::path(
    get,
    path = "/api/browser/page",
    tag = "browser",
    responses(
        (status = 200, description = "Page information", body = PageInfoResponse)
    )
)]
pub async fn page_info_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Browser error: {}", e)})),
            );
        }
    };

    let url = adapter.current_url().await.unwrap_or_default();
    let title = adapter
        .eval("document.title")
        .await
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!(PageInfoResponse { url, title })),
    )
}

// ============================================================================
// Tabs
// ============================================================================

/// List open tabs
#[utoipa::path(
    get,
    path = "/api/browser/tabs",
    tag = "browser",
    responses(
        (status = 200, description = "List of open tabs", body = TabListResponse)
    )
)]
pub async fn tab_list_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(serde_json::json!({"error": format!("Browser error: {}", e)}));
        }
    };

    match adapter.list_tabs().await {
        Ok(tabs) => {
            let tab_list: Vec<TabInfoResponse> = tabs
                .iter()
                .map(|t| TabInfoResponse {
                    index: t.index,
                    url: t.url.clone(),
                    active: t.active,
                })
                .collect();
            Json(serde_json::json!(TabListResponse { tabs: tab_list }))
        }
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// Open a new tab
#[utoipa::path(
    post,
    path = "/api/browser/tabs",
    tag = "browser",
    request_body = TabNewRequest,
    responses(
        (status = 200, description = "Tab opened", body = TabNewResponse)
    )
)]
pub async fn tab_new_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<TabNewRequest>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(TabNewResponse {
                success: false,
                index: None,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    match adapter.new_tab(request.url.as_deref()).await {
        Ok(index) => Json(TabNewResponse {
            success: true,
            index: Some(index),
            error: None,
        }),
        Err(e) => Json(TabNewResponse {
            success: false,
            index: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Switch to a tab
#[utoipa::path(
    post,
    path = "/api/browser/tabs/switch",
    tag = "browser",
    request_body = TabSwitchRequest,
    responses(
        (status = 200, description = "Tab switched", body = BrowserActionResponse)
    )
)]
pub async fn tab_switch_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<TabSwitchRequest>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(BrowserActionResponse {
                success: false,
                result: None,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    match adapter.switch_tab(request.index).await {
        Ok(()) => Json(BrowserActionResponse {
            success: true,
            result: None,
            error: None,
        }),
        Err(e) => Json(BrowserActionResponse {
            success: false,
            result: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Close a tab
#[utoipa::path(
    delete,
    path = "/api/browser/tabs/{index}",
    tag = "browser",
    params(
        ("index" = usize, Path, description = "Tab index to close")
    ),
    responses(
        (status = 200, description = "Tab closed", body = BrowserActionResponse)
    )
)]
pub async fn tab_close_handler(
    State(state): State<Arc<ServerState>>,
    Path(index): Path<usize>,
) -> impl IntoResponse {
    let adapter = match state.get_page().await {
        Ok(a) => a,
        Err(e) => {
            return Json(BrowserActionResponse {
                success: false,
                result: None,
                error: Some(format!("Browser error: {}", e)),
            });
        }
    };

    match adapter.close_tab(index).await {
        Ok(()) => Json(BrowserActionResponse {
            success: true,
            result: None,
            error: None,
        }),
        Err(e) => Json(BrowserActionResponse {
            success: false,
            result: None,
            error: Some(e.to_string()),
        }),
    }
}

// ============================================================================
// PDF Export
// ============================================================================

/// Export page as PDF
#[utoipa::path(
    get,
    path = "/api/browser/pdf",
    tag = "browser",
    responses(
        (status = 200, description = "PDF document", content_type = "application/pdf"),
        (status = 500, description = "Browser error")
    )
)]
pub async fn pdf_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
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

    match adapter.pdf().await {
        Ok(bytes) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/pdf")],
            bytes,
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            format!(r#"{{"error":"{}"}}"#, e).into_bytes(),
        ),
    }
}

// ============================================================================
// Debug
// ============================================================================

/// Clean debug output directory
#[utoipa::path(
    post,
    path = "/api/debug/cleanup",
    tag = "debug",
    responses(
        (status = 200, description = "Cleanup completed", body = DebugCleanupResponse),
        (status = 500, description = "Cleanup failed")
    )
)]
pub async fn debug_cleanup_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let debug_dir = state.debug_dir();
    let policy = crate::utils::CleanupPolicy::default();

    match crate::utils::cleanup_debug_dir(&debug_dir, &policy) {
        Ok(stats) => Json(DebugCleanupResponse {
            success: true,
            files_removed: stats.files_removed,
            bytes_freed: stats.bytes_freed,
            files_remaining: stats.files_remaining,
            error: None,
        }),
        Err(e) => Json(DebugCleanupResponse {
            success: false,
            files_removed: 0,
            bytes_freed: 0,
            files_remaining: 0,
            error: Some(e.to_string()),
        }),
    }
}

// ============================================================================
// Sessions
// ============================================================================

/// Create a new session
#[utoipa::path(
    post,
    path = "/api/sessions",
    tag = "sessions",
    request_body = CreateSessionRequest,
    responses(
        (status = 200, description = "Session created", body = CreateSessionResponse),
        (status = 400, description = "Failed to create session")
    )
)]
pub async fn create_session_handler(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    match state.create_session(request.name, request.keep_alive).await {
        Ok(id) => Json(CreateSessionResponse {
            success: true,
            id: Some(id),
            error: None,
        }),
        Err(e) => Json(CreateSessionResponse {
            success: false,
            id: None,
            error: Some(e),
        }),
    }
}

/// List all sessions
#[utoipa::path(
    get,
    path = "/api/sessions",
    tag = "sessions",
    responses(
        (status = 200, description = "List of active sessions", body = SessionListResponse)
    )
)]
pub async fn list_sessions_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let sessions = state.list_sessions().await;

    let session_list: Vec<SessionInfo> = sessions
        .into_iter()
        .map(|s| SessionInfo {
            id: s.id,
            name: s.name,
            created_at: s.created_at.to_rfc3339(),
            last_activity: s.last_activity.to_rfc3339(),
            keep_alive: s.keep_alive,
        })
        .collect();

    Json(SessionListResponse {
        sessions: session_list,
    })
}

/// Get session by ID
#[utoipa::path(
    get,
    path = "/api/sessions/{id}",
    tag = "sessions",
    params(
        ("id" = String, Path, description = "Session ID")
    ),
    responses(
        (status = 200, description = "Session details", body = SessionInfo),
        (status = 404, description = "Session not found")
    )
)]
pub async fn get_session_handler(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.get_session(&id).await {
        Some(session) => (
            StatusCode::OK,
            Json(serde_json::json!(SessionInfo {
                id: session.id,
                name: session.name,
                created_at: session.created_at.to_rfc3339(),
                last_activity: session.last_activity.to_rfc3339(),
                keep_alive: session.keep_alive,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Session '{}' not found", id)})),
        ),
    }
}

/// Delete a session
#[utoipa::path(
    delete,
    path = "/api/sessions/{id}",
    tag = "sessions",
    params(
        ("id" = String, Path, description = "Session ID")
    ),
    responses(
        (status = 200, description = "Session deleted", body = DeleteSessionResponse),
        (status = 404, description = "Session not found")
    )
)]
pub async fn delete_session_handler(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.close_session(&id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(DeleteSessionResponse {
                success: true,
                error: None,
            }),
        ),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(DeleteSessionResponse {
                success: false,
                error: Some(e),
            }),
        ),
    }
}

// ============================================================================
// Executions
// ============================================================================

/// List recent executions
#[utoipa::path(
    get,
    path = "/api/executions",
    tag = "executions",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of executions to return")
    ),
    responses(
        (status = 200, description = "List of executions", body = ExecutionListResponse)
    )
)]
pub async fn list_executions_handler(
    State(state): State<Arc<ServerState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params.get("limit").and_then(|l| l.parse::<usize>().ok());

    let executions = state.list_executions(limit).await;

    let execution_list: Vec<ExecutionInfo> = executions
        .into_iter()
        .map(|e| ExecutionInfo {
            id: e.id,
            workflow: e.workflow,
            status: e.status,
            started_at: e.started_at.to_rfc3339(),
            completed_at: e.completed_at.map(|t| t.to_rfc3339()),
            duration_ms: e.duration_ms,
            steps_executed: e.steps_executed,
            total_steps: e.total_steps,
            error: e.error,
        })
        .collect();

    Json(ExecutionListResponse {
        executions: execution_list,
    })
}

/// Get execution details
#[utoipa::path(
    get,
    path = "/api/executions/{id}",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID")
    ),
    responses(
        (status = 200, description = "Execution details", body = ExecutionDetail),
        (status = 404, description = "Execution not found")
    )
)]
pub async fn get_execution_handler(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.get_execution(&id).await {
        Some(e) => (
            StatusCode::OK,
            Json(serde_json::json!(ExecutionDetail {
                info: ExecutionInfo {
                    id: e.id,
                    workflow: e.workflow,
                    status: e.status,
                    started_at: e.started_at.to_rfc3339(),
                    completed_at: e.completed_at.map(|t| t.to_rfc3339()),
                    duration_ms: e.duration_ms,
                    steps_executed: e.steps_executed,
                    total_steps: e.total_steps,
                    error: e.error,
                },
                output: e.output,
                params: e.params,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Execution '{}' not found", id)})),
        ),
    }
}

/// Cancel an execution
#[utoipa::path(
    delete,
    path = "/api/executions/{id}",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID")
    ),
    responses(
        (status = 200, description = "Execution cancelled"),
        (status = 404, description = "Execution not found"),
        (status = 400, description = "Cannot cancel execution")
    )
)]
pub async fn cancel_execution_handler(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.cancel_execution(&id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"success": true}))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        ),
    }
}
