//! Streamable HTTP Transport for MCP Server
//!
//! Implements the MCP 2025-03-26 Streamable HTTP specification:
//! - POST /mcp: Client sends JSON-RPC requests, server responds via SSE or JSON
//! - GET /mcp: Client opens SSE stream for server-initiated messages
//! - DELETE /mcp: Client terminates session
//!
//! This transport enables CCOS persistence across client sessions.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures::stream::unfold;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use super::server::{MCPError, MCPRequest, MCPResponse, MCPServer};
use crate::approval::{
    queue::ApprovalAuthority,
    storage_file::FileApprovalStorage,
    types::{ApprovalCategory, ApprovalRequest},
    UnifiedApprovalQueue,
};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::secrets::SecretStore;
use crate::utils::fs::get_workspace_root;

/// Session state for a connected client
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Shared state for the HTTP transport
pub struct HttpTransportState {
    /// The underlying MCP server with tools
    pub server: Arc<MCPServer>,
    /// Active sessions
    pub sessions: RwLock<HashMap<String, Session>>,
    /// Broadcast channel for server-initiated messages (per session)
    pub broadcasters: RwLock<HashMap<String, broadcast::Sender<Value>>>,
    /// Approval queue for secrets and other approvals
    pub approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    pub marketplace: Option<Arc<CapabilityMarketplace>>,
    /// Broadcast channel for shutdown notification
    pub shutdown_tx: broadcast::Sender<()>,
}

impl HttpTransportState {
    pub fn new(server: MCPServer) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            server: Arc::new(server),
            sessions: RwLock::new(HashMap::new()),
            broadcasters: RwLock::new(HashMap::new()),
            approval_queue: None,
            marketplace: None,
            shutdown_tx,
        }
    }

    /// Notify all listeners that the server is shutting down
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    pub fn with_approval_queue(mut self, queue: UnifiedApprovalQueue<FileApprovalStorage>) -> Self {
        self.approval_queue = Some(queue);
        self
    }

    pub fn with_marketplace(mut self, marketplace: Arc<CapabilityMarketplace>) -> Self {
        self.marketplace = Some(marketplace);
        self
    }

    /// Create a new session and return its ID
    pub async fn create_session(&self) -> String {
        let session_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now();

        let session = Session {
            id: session_id.clone(),
            created_at: now,
            last_activity: now,
        };

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);

        // Create a broadcast channel for this session
        let (tx, _) = broadcast::channel(100);
        self.broadcasters
            .write()
            .await
            .insert(session_id.clone(), tx);

        session_id
    }

    /// Validate and refresh a session
    pub async fn validate_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.last_activity = chrono::Utc::now();
            true
        } else {
            false
        }
    }

    /// Terminate a session
    pub async fn terminate_session(&self, session_id: &str) -> bool {
        let removed = self.sessions.write().await.remove(session_id).is_some();
        self.broadcasters.write().await.remove(session_id);
        removed
    }

    /// Get broadcaster for a session
    pub async fn get_broadcaster(&self, session_id: &str) -> Option<broadcast::Sender<Value>> {
        self.broadcasters.read().await.get(session_id).cloned()
    }
}

/// HTTP Transport configuration
#[derive(Debug, Clone)]
pub struct HttpTransportConfig {
    pub host: String,
    pub port: u16,
    pub keep_alive_secs: u64,
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            keep_alive_secs: 30,
        }
    }
}

/// Run the MCP server with Streamable HTTP transport
pub async fn run_http_transport(
    server: MCPServer,
    config: HttpTransportConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_http_transport_with_approvals(server, config, None, None).await
}

/// Run the MCP server with Streamable HTTP transport and optional approval queue
pub async fn run_http_transport_with_approvals(
    server: MCPServer,
    config: HttpTransportConfig,
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    marketplace: Option<Arc<CapabilityMarketplace>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut state = HttpTransportState::new(server);
    if let Some(queue) = approval_queue {
        state = state.with_approval_queue(queue);
    }
    if let Some(mp) = marketplace {
        state = state.with_marketplace(mp);
    }
    let state = Arc::new(state);

    let app = Router::new()
        .route("/mcp", post(handle_post))
        .route("/mcp", get(handle_get))
        .route("/mcp", delete(handle_delete))
        .route("/health", get(handle_health))
        // Approval UI routes
        .route("/approvals", get(handle_approvals_html))
        .route("/secrets", get(handle_secrets_html))
        .route("/api/approvals", get(handle_api_approvals_list))
        .route("/api/secrets", get(handle_api_secrets_list))
        .route(
            "/api/approvals/:id/approve",
            post(handle_api_approval_approve),
        )
        .route(
            "/api/approvals/:id/reject",
            post(handle_api_approval_reject),
        )
        .route(
            "/api/approvals/:id/reapprove",
            post(handle_api_approval_reapprove),
        )
        .with_state(state.clone());

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    eprintln!("Starting CCOS MCP server on http://{}/mcp", addr);
    eprintln!("  POST /mcp - Send JSON-RPC requests");
    eprintln!("  GET /mcp  - Open SSE stream");
    eprintln!("  DELETE /mcp - Terminate session");
    eprintln!("  GET /approvals - Approval web UI");
    eprintln!("  GET /api/approvals - List pending approvals (JSON)");

    let listener = TcpListener::bind(addr).await?;
    let transport_state = state.clone();
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            transport_state.shutdown();
        })
        .await?;

    eprintln!("CCOS MCP server stopped");
    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = signal::ctrl_c().await {
        eprintln!("Failed to install Ctrl+C handler: {}", err);
        return;
    }
    eprintln!("Ctrl+C received, shutting down...");
}

/// Health check endpoint
async fn handle_health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "server": "ccos-mcp" }))
}

/// POST /mcp - Handle JSON-RPC requests from client
async fn handle_post(
    State(state): State<Arc<HttpTransportState>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    // Check Accept header
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let supports_sse = accept.contains("text/event-stream");
    let supports_json = accept.contains("application/json") || accept.is_empty();

    if !supports_sse && !supports_json {
        return (
            StatusCode::NOT_ACCEPTABLE,
            Json(json!({ "error": "Accept header must include application/json or text/event-stream" })),
        )
            .into_response();
    }

    // Get or validate session
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Parse request(s)
    let requests: Vec<MCPRequest> = if request.is_array() {
        match serde_json::from_value(request.clone()) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Parse error: {}", e) })),
                )
                    .into_response();
            }
        }
    } else {
        match serde_json::from_value::<MCPRequest>(request.clone()) {
            Ok(r) => vec![r],
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Parse error: {}", e) })),
                )
                    .into_response();
            }
        }
    };

    // Check if this is initialization
    let is_init = requests.iter().any(|r| r.method == "initialize");

    // Validate session for non-init requests
    if !is_init {
        if let Some(ref sid) = session_id {
            if !state.validate_session(sid).await {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "Session not found or expired" })),
                )
                    .into_response();
            }
        }
    }

    // Process requests
    let mut responses: Vec<MCPResponse> = Vec::new();
    let mut new_session_id: Option<String> = None;

    for req in requests {
        let response = process_request(&state, &req).await;

        // If this is initialize, create a session
        if req.method == "initialize" && response.error.is_none() {
            new_session_id = Some(state.create_session().await);
        }

        // Only include response if request had an ID (not a notification)
        if req.id.is_some() {
            responses.push(response);
        }
    }

    // Build response
    if responses.is_empty() {
        // All notifications - return 202 Accepted
        return StatusCode::ACCEPTED.into_response();
    }

    // For simplicity, return JSON (not SSE) for now
    // Full SSE streaming for long-running requests would be added later
    let mut response_builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json");

    // Add session ID header if new session was created
    if let Some(sid) = new_session_id {
        response_builder = response_builder.header("Mcp-Session-Id", sid);
    }

    let body = if responses.len() == 1 {
        serde_json::to_string(&responses[0]).unwrap_or_default()
    } else {
        serde_json::to_string(&responses).unwrap_or_default()
    };

    response_builder.body(Body::from(body)).unwrap()
}

/// GET /mcp - Open SSE stream for server-initiated messages
async fn handle_get(State(state): State<Arc<HttpTransportState>>, headers: HeaderMap) -> Response {
    // Check Accept header
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !accept.contains("text/event-stream") {
        return (
            StatusCode::NOT_ACCEPTABLE,
            Json(json!({ "error": "Accept header must include text/event-stream" })),
        )
            .into_response();
    }

    // Get session ID
    let session_id = match headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
        Some(sid) => sid.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing Mcp-Session-Id header" })),
            )
                .into_response();
        }
    };

    // Validate session
    if !state.validate_session(&session_id).await {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Session not found or expired" })),
        )
            .into_response();
    }

    // Get broadcaster for this session
    let broadcaster = match state.get_broadcaster(&session_id).await {
        Some(b) => b,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Session broadcaster not found" })),
            )
                .into_response();
        }
    };

    // Create SSE stream using futures::stream::unfold
    let rx = broadcaster.subscribe();
    let shutdown_rx = state.shutdown_tx.subscribe();

    let stream = unfold(
        (rx, shutdown_rx, session_id.clone()),
        |(mut rx, mut shutdown_rx, sid)| async move {
            loop {
                let sid_clone = sid.clone();
                tokio::select! {
                    msg = rx.recv() => {
                        match msg {
                            Ok(msg) => {
                                let data = serde_json::to_string(&msg).unwrap_or_default();
                                let event = Event::default().event("message").data(data);
                                return Some((Ok::<_, Infallible>(event), (rx, shutdown_rx, sid)));
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                // Client is slow, skip missed messages
                                continue;
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                // Channel closed, end stream
                                return None;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        eprintln!("[http_transport] Shutdown received, closing SSE stream for session {}", sid_clone);
                        return None;
                    }
                }
            }
        },
    );

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// DELETE /mcp - Terminate session
async fn handle_delete(
    State(state): State<Arc<HttpTransportState>>,
    headers: HeaderMap,
) -> Response {
    let session_id = match headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
        Some(sid) => sid,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing Mcp-Session-Id header" })),
            )
                .into_response();
        }
    };

    if state.terminate_session(session_id).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Session not found" })),
        )
            .into_response()
    }
}

/// Process a single JSON-RPC request
async fn process_request(state: &HttpTransportState, request: &MCPRequest) -> MCPResponse {
    let result = match request.method.as_str() {
        "initialize" => handle_initialize(state),
        "initialized" => Ok(json!({})), // Notification acknowledgment
        "tools/list" => handle_tools_list(state),
        "tools/call" => handle_tools_call(state, &request.params).await,
        "ping" => Ok(json!({ "pong": true })),
        _ => Err(format!("Method not found: {}", request.method)),
    };

    match result {
        Ok(result) => MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(result),
            error: None,
        },
        Err(e) => MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: None,
            error: Some(MCPError {
                code: -32603,
                message: e,
                data: None,
            }),
        },
    }
}

fn handle_initialize(_state: &HttpTransportState) -> Result<Value, String> {
    Ok(json!({
        "protocolVersion": "2025-03-26",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "ccos-mcp",
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}

fn handle_tools_list(state: &HttpTransportState) -> Result<Value, String> {
    // Get tool definitions from the server
    let tools: Vec<Value> = state
        .server
        .get_tools()
        .iter()
        .map(|def| {
            json!({
                "name": def.name,
                "description": def.description,
                "inputSchema": def.input_schema
            })
        })
        .collect();

    Ok(json!({ "tools": tools }))
}

async fn handle_tools_call(state: &HttpTransportState, params: &Value) -> Result<Value, String> {
    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing tool name".to_string())?;

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    let result = state
        .server
        .call_tool(tool_name, arguments)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&result).unwrap_or_default()
        }]
    }))
}

// ============================================================================
// Approval UI Handlers
// ============================================================================

/// GET /approvals - HTML page with pending approvals
async fn handle_approvals_html(State(state): State<Arc<HttpTransportState>>) -> impl IntoResponse {
    let pending = if let Some(ref queue) = state.approval_queue {
        queue.list_pending().await.unwrap_or_default()
    } else {
        vec![]
    };

    let html = generate_approvals_html(&pending);
    Html(html)
}

/// Check for X-Admin-Token header if CCOS_ADMIN_TOKEN is set
fn check_admin_token(headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
    let expected_token = std::env::var("CCOS_ADMIN_TOKEN").unwrap_or_default();
    if expected_token.is_empty() {
        return Ok(()); // Security disabled if no token set
    }

    let token = headers
        .get("X-Admin-Token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim());

    match token {
        Some(t) if t == expected_token => Ok(()),
        _ => Err((StatusCode::UNAUTHORIZED, "Invalid or missing X-Admin-Token")),
    }
}

/// GET /api/approvals - JSON list of all approvals (with status)
async fn handle_api_approvals_list(
    State(state): State<Arc<HttpTransportState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err((status, msg)) = check_admin_token(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }

    // Get all approvals (not just pending)
    let all_approvals = if let Some(ref queue) = state.approval_queue {
        queue
            .list(crate::approval::types::ApprovalFilter::default())
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    let approvals: Vec<Value> = all_approvals
        .iter()
        .map(|req| {
            let (category_type, details) = match &req.category {
                ApprovalCategory::ServerDiscovery {
                    server_info,
                    domain_match,
                    requesting_goal,
                    ..
                } => (
                    "ServerDiscovery",
                    json!({
                        "server_name": server_info.name,
                        "endpoint": server_info.endpoint,
                        "domain_match": domain_match,
                        "requesting_goal": requesting_goal
                    }),
                ),
                ApprovalCategory::EffectApproval {
                    capability_id,
                    effects,
                    intent_description,
                } => (
                    "EffectApproval",
                    json!({
                        "capability_id": capability_id,
                        "effects": effects,
                        "intent_description": intent_description
                    }),
                ),
                ApprovalCategory::SynthesisApproval {
                    capability_id,
                    is_pure,
                    ..
                } => (
                    "SynthesisApproval",
                    json!({
                        "capability_id": capability_id,
                        "is_pure": is_pure
                    }),
                ),
                ApprovalCategory::LlmPromptApproval {
                    prompt,
                    risk_reasons,
                } => (
                    "LlmPromptApproval",
                    json!({
                        "prompt_preview": prompt.chars().take(200).collect::<String>(),
                        "risk_reasons": risk_reasons
                    }),
                ),
                ApprovalCategory::SecretRequired {
                    capability_id,
                    secret_type,
                    description,
                } => (
                    "SecretRequired",
                    json!({
                        "capability_id": capability_id,
                        "secret_type": secret_type,
                        "description": description
                    }),
                ),
                ApprovalCategory::BudgetExtension {
                    plan_id,
                    intent_id,
                    dimension,
                    requested_additional,
                    consumed,
                    limit,
                } => (
                    "BudgetExtension",
                    json!({
                        "plan_id": plan_id,
                        "intent_id": intent_id,
                        "dimension": dimension,
                        "requested_additional": requested_additional,
                        "consumed": consumed,
                        "limit": limit
                    }),
                ),
            };

            // Determine status string for UI filtering
            let status = if req.status.is_pending() {
                "pending"
            } else if req.status.is_rejected() {
                "rejected"
            } else if matches!(
                req.status,
                crate::approval::types::ApprovalStatus::Expired { .. }
            ) {
                "expired"
            } else {
                "approved"
            };

            json!({
                "id": req.id,
                "status": status,
                "category_type": category_type,
                "details": details,
                "requested_at": req.requested_at.to_rfc3339(),
                "expires_at": req.expires_at.to_rfc3339()
            })
        })
        .collect();

    Json(json!({ "approvals": approvals })).into_response()
}

/// Request body for approval with secret
#[derive(serde::Deserialize)]
struct ApproveSecretRequest {
    #[serde(default)]
    secret_value: Option<String>,
    #[serde(default)]
    provided_env_var: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// POST /api/approvals/:id/approve - Approve a request
async fn handle_api_approval_approve(
    State(state): State<Arc<HttpTransportState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ApproveSecretRequest>,
) -> impl IntoResponse {
    if let Err((status, msg)) = check_admin_token(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }

    if let Some(ref queue) = state.approval_queue {
        // Note: SecretRequired handling was removed as the feature was deprecated

        // NEW: Handle secret saving
        if let (Some(req), Some(val)) = (
            queue.get(&id).await.ok().flatten(),
            body.secret_value.clone(),
        ) {
            if let crate::approval::types::ApprovalCategory::SecretRequired {
                secret_type, ..
            } = req.category
            {
                let workspace_root = crate::utils::fs::get_workspace_root();
                // Use SecretStore to save
                match crate::secrets::SecretStore::new(Some(workspace_root)) {
                    Ok(mut store) => {
                        if let Err(e) = store.set_local(&secret_type, val) {
                            log::error!("Failed to save secret: {}", e);
                            return Json(
                                json!({ "error": format!("Failed to save secret: {}", e) }),
                            )
                            .into_response();
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to load secret store: {}", e);
                        return Json(
                            json!({ "error": format!("Failed to load secret store: {}", e) }),
                        )
                        .into_response();
                    }
                }
            }
        }

        // Approve the request
        //
        // IMPORTANT: For ServerDiscovery approvals, use `approve_server` so that
        // versioning (archiving old approved versions) and filesystem moves
        // (pending/ -> approved/) are performed.
        let req_for_category = queue.get(&id).await.ok().flatten();
        let approve_result = if let Some(req) = req_for_category {
            match req.category {
                crate::approval::types::ApprovalCategory::ServerDiscovery { .. } => {
                    queue.approve_server(
                        &id,
                        ApprovalAuthority::User("web".to_string()),
                        body.reason,
                    )
                    .await
                }
                _ => {
                    queue.approve(&id, ApprovalAuthority::User("web".to_string()), body.reason)
                        .await
                }
            }
        } else {
            Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "Approval request not found: {}",
                id
            )))
        };

        match approve_result {
            Ok(()) => {
                // NEW: If this was a server discovery approval, automatically register the server
                if let (Some(req), Some(mp)) =
                    (queue.get(&id).await.ok().flatten(), &state.marketplace)
                {
                    if let crate::approval::types::ApprovalCategory::ServerDiscovery {
                        ref server_info,
                        ..
                    } = req.category
                    {
                        let server_name = server_info.name.clone();
                        let server_id = crate::utils::fs::sanitize_filename(&server_name);
                        let workspace_root = crate::utils::fs::get_workspace_root();
                        let approved_dir = workspace_root
                            .join("capabilities/servers/approved")
                            .join(&server_id);

                        if approved_dir.exists() {
                            let mp_clone = mp.clone();
                            tokio::spawn(async move {
                                match mp_clone
                                    .import_capabilities_from_rtfs_dir_recursive(&approved_dir)
                                    .await
                                {
                                    Ok(count) => {
                                        log::info!(
                                            "[http_transport] Automatically loaded {} capabilities for {}",
                                            count,
                                            server_name
                                        );
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "[http_transport] Error auto-loading capabilities for {}: {}",
                                            server_name,
                                            e
                                        );
                                    }
                                }
                            });
                        }
                    }
                }

                (StatusCode::OK, Json(json!({ "success": true, "id": id }))).into_response()
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response(),
        }
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Approval queue not configured" })),
        )
            .into_response()
    }
}

/// Request body for rejection
#[derive(serde::Deserialize)]
struct RejectRequest {
    reason: String,
}

/// POST /api/approvals/:id/reject - Reject a request
async fn handle_api_approval_reject(
    State(state): State<Arc<HttpTransportState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<RejectRequest>,
) -> impl IntoResponse {
    if let Err((status, msg)) = check_admin_token(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }

    if let Some(ref queue) = state.approval_queue {
        match queue
            .reject(&id, ApprovalAuthority::User("web".to_string()), body.reason)
            .await
        {
            Ok(()) => (StatusCode::OK, Json(json!({ "success": true, "id": id }))).into_response(),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response(),
        }
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Approval queue not configured" })),
        )
            .into_response()
    }
}

/// POST /api/approvals/:id/reapprove - Re-approve a rejected or expired request
async fn handle_api_approval_reapprove(
    State(state): State<Arc<HttpTransportState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err((status, msg)) = check_admin_token(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }

    if let Some(ref queue) = state.approval_queue {
        // Re-approve by calling approve on the existing request
        match queue
            .approve(
                &id,
                ApprovalAuthority::User("web".to_string()),
                Some("Re-approved via web UI".to_string()),
            )
            .await
        {
            Ok(()) => {
                // Also try to load capabilities if this was a server discovery
                if let (Some(req), Some(mp)) =
                    (queue.get(&id).await.ok().flatten(), &state.marketplace)
                {
                    if let crate::approval::types::ApprovalCategory::ServerDiscovery {
                        ref server_info,
                        ..
                    } = req.category
                    {
                        let server_name = server_info.name.clone();
                        let server_id = crate::utils::fs::sanitize_filename(&server_name);
                        let workspace_root = crate::utils::fs::get_workspace_root();
                        let approved_dir = workspace_root
                            .join("capabilities/servers/approved")
                            .join(&server_id);

                        if approved_dir.exists() {
                            let mp_clone = mp.clone();
                            tokio::spawn(async move {
                                match mp_clone
                                    .import_capabilities_from_rtfs_dir_recursive(&approved_dir)
                                    .await
                                {
                                    Ok(count) => {
                                        log::info!(
                                            "[http_transport] Re-approved and loaded {} capabilities for {}",
                                            count,
                                            server_name
                                        );
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "[http_transport] Error loading capabilities for {}: {}",
                                            server_name,
                                            e
                                        );
                                    }
                                }
                            });
                        }
                    }
                }
                (
                    StatusCode::OK,
                    Json(
                        json!({ "success": true, "id": id, "message": "Re-approved successfully" }),
                    ),
                )
                    .into_response()
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response(),
        }
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Approval queue not configured" })),
        )
            .into_response()
    }
}

/// GET /api/secrets - List all approved secrets (names only)
async fn handle_api_secrets_list() -> impl IntoResponse {
    let store = SecretStore::new(Some(get_workspace_root()))
        .unwrap_or_else(|_| SecretStore::new(None).expect("Failed to create SecretStore"));

    let local = store.list_local();
    let mut mappings = HashMap::new();
    for name in store.list_mappings() {
        if let Some(target) = store.get_mapping(name) {
            mappings.insert(name.to_string(), target.clone());
        }
    }

    Json(json!({
        "secrets": local,
        "mappings": mappings
    }))
}

/// GET /secrets - HTML page for approved secrets
async fn handle_secrets_html() -> impl IntoResponse {
    let store = SecretStore::new(Some(get_workspace_root()))
        .unwrap_or_else(|_| SecretStore::new(None).expect("Failed to create SecretStore"));

    let local = store.list_local();
    let mut mappings = Vec::new();
    for name in store.list_mappings() {
        if let Some(target) = store.get_mapping(name) {
            mappings.push((name.to_string(), target.clone()));
        }
    }

    let mut secrets_html = String::new();

    if local.is_empty() && mappings.is_empty() {
        secrets_html.push_str("<p class='empty'>No secrets approved yet.</p>");
    } else {
        if !local.is_empty() {
            secrets_html.push_str("<h2>Local Secrets</h2><ul>");
            for name in local {
                secrets_html.push_str(&format!(
                    "<li><code>{}</code> (Value stored locally)</li>",
                    name
                ));
            }
            secrets_html.push_str("</ul>");
        }

        if !mappings.is_empty() {
            secrets_html.push_str("<h2>Mapped Secrets</h2><ul>");
            for (name, target) in mappings {
                secrets_html.push_str(&format!(
                    "<li><code>{}</code> &rarr; Environment Variable: <code>{}</code></li>",
                    name, target
                ));
            }
            secrets_html.push_str("</ul>");
        }
    }

    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>🔐 CCOS Approved Secrets</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; background: #0d1117; color: #c9d1d9; max-width: 800px; margin: 40px auto; padding: 0 20px; }}
        h1 {{ color: #58a6ff; border-bottom: 1px solid #30363d; padding-bottom: 15px; }}
        h2 {{ color: #8b949e; margin-top: 30px; font-size: 1.2em; }}
        ul {{ list-style-type: none; padding: 0; }}
        li {{ background: #161b22; border: 1px solid #30363d; border-radius: 6px; margin-bottom: 10px; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; }}
        code {{ background: #21262d; padding: 2px 6px; border-radius: 4px; color: #79c0ff; font-family: ui-monospace, SFMono-Regular, SF Mono, Menlo, Consolas, Liberation Mono, monospace; }}
        .empty {{ color: #8b949e; font-style: italic; text-align: center; margin-top: 50px; }}
        a.nav {{ color: #58a6ff; text-decoration: none; font-size: 0.9em; }}
        a.nav:hover {{ text-decoration: underline; }}
    </style>
</head>
<body>
    <div style="display: flex; justify-content: space-between; align-items: center;">
        <h1>🔐 Approved Secrets</h1>
        <a href="/approvals" class="nav">&larr; Back to Approvals</a>
    </div>
    {}
</body>
</html>"#,
        secrets_html
    ))
}

/// Generate HTML page for approvals with tabs and auto-refresh
fn generate_approvals_html(_pending: &[ApprovalRequest]) -> String {
    // Generate a fully client-side rendered page that uses the API
    r#"<!DOCTYPE html>
<html>
<head>
    <title>CCOS Approvals</title>
    <style>
        body { font-family: -apple-system, system-ui, sans-serif; max-width: 900px; margin: 0 auto; padding: 20px; background: #0d1117; color: #c9d1d9; }
        h1 { color: #58a6ff; display: flex; justify-content: space-between; align-items: center; }
        .tabs { display: flex; gap: 8px; margin-bottom: 20px; border-bottom: 1px solid #30363d; padding-bottom: 8px; }
        .tab { padding: 8px 16px; border: none; background: transparent; color: #8b949e; cursor: pointer; border-radius: 4px 4px 0 0; font-size: 14px; }
        .tab:hover { background: #21262d; color: #c9d1d9; }
        .tab.active { background: #238636; color: white; }
        .tab .count { background: #30363d; padding: 2px 8px; border-radius: 10px; margin-left: 6px; font-size: 12px; }
        .tab.active .count { background: #2ea043; }
        .approval-card { background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 16px; margin-bottom: 16px; }
        .approval-card h3 { margin-top: 0; color: #f0f6fc; display: flex; align-items: center; gap: 8px; }
        .approval-card p { margin: 8px 0; color: #8b949e; }
        .approval-card .status-badge { font-size: 12px; padding: 2px 8px; border-radius: 4px; text-transform: uppercase; }
        .status-pending { background: #1f6feb; color: white; }
        .status-rejected { background: #da3633; color: white; }
        .status-expired { background: #6e7681; color: white; }
        .status-approved { background: #238636; color: white; }
        .secret-input { margin: 12px 0; }
        .secret-input label { display: block; margin-bottom: 4px; color: #8b949e; }
        .secret-input input { width: 100%; padding: 8px; border: 1px solid #30363d; border-radius: 4px; background: #0d1117; color: #c9d1d9; box-sizing: border-box; }
        .actions { margin-top: 12px; display: flex; gap: 8px; flex-wrap: wrap; }
        .actions button { padding: 8px 16px; border: none; border-radius: 4px; cursor: pointer; font-weight: 500; }
        .approve { background: #238636; color: white; }
        .approve:hover { background: #2ea043; }
        .reject { background: #da3633; color: white; }
        .reject:hover { background: #f85149; }
        .reapprove { background: #1f6feb; color: white; }
        .reapprove:hover { background: #388bfd; }
        .empty { color: #8b949e; font-style: italic; text-align: center; padding: 40px; }
        .success { color: #3fb950; }
        .error { color: #f85149; }
        .refresh-indicator { font-size: 12px; color: #8b949e; display: flex; align-items: center; gap: 6px; }
        .refresh-indicator .dot { width: 8px; height: 8px; border-radius: 50%; background: #238636; animation: pulse 2s infinite; }
        @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.5; } }
        .nav-link { color: #58a6ff; text-decoration: none; font-size: 14px; }
        .nav-link:hover { text-decoration: underline; }
        .header-actions { display: flex; gap: 16px; align-items: center; }
        .layout { display: grid; grid-template-columns: 2fr 1fr; gap: 16px; }
        .main-pane { min-width: 0; }
        .side-pane { min-width: 240px; }
        .panel { background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 16px; }
        .panel h2 { margin: 0 0 12px 0; color: #f0f6fc; font-size: 16px; }
        .panel .panel-counts { display: flex; gap: 8px; margin-bottom: 12px; }
        .panel .panel-counts span { background: #21262d; border-radius: 999px; padding: 2px 8px; font-size: 12px; color: #c9d1d9; }
        .panel .budget-item { border-top: 1px solid #30363d; padding: 10px 0; }
        .panel .budget-item:first-of-type { border-top: none; padding-top: 0; }
        .panel .budget-item small { color: #8b949e; display: block; margin-top: 4px; }
        .panel .budget-item .status-tag { font-size: 11px; padding: 2px 6px; border-radius: 4px; text-transform: uppercase; margin-left: 6px; }
        .panel .status-pending { background: #1f6feb; color: white; }
        .panel .status-approved { background: #238636; color: white; }
        .panel .status-rejected { background: #da3633; color: white; }
        .panel .status-expired { background: #6e7681; color: white; }
        @media (max-width: 960px) {
            .layout { grid-template-columns: 1fr; }
        }
    </style>
</head>
<body>
    <h1>
        <span>🔐 CCOS Approvals</span>
        <div class="header-actions">
            <div class="refresh-indicator"><span class="dot"></span> Auto-refresh</div>
            <a href="/secrets" class="nav-link">🔑 Secrets</a>
        </div>
    </h1>
    <div class="layout">
        <div class="main-pane">
            <div class="tabs">
                <button class="tab active" data-status="pending" onclick="switchTab('pending')">⏳ Pending <span class="count" id="count-pending">0</span></button>
                <button class="tab" data-status="rejected" onclick="switchTab('rejected')">❌ Rejected <span class="count" id="count-rejected">0</span></button>
                <button class="tab" data-status="expired" onclick="switchTab('expired')">⏰ Expired <span class="count" id="count-expired">0</span></button>
            </div>
            <div id="approvals"></div>
        </div>
        <aside class="side-pane">
            <div class="panel" id="budget-panel">
                <h2>Budget Extensions</h2>
                <p class="empty">Loading budget approvals...</p>
            </div>
        </aside>
    </div>
    <script>
        let currentTab = 'pending';
        let allApprovals = [];
        
        function switchTab(status) {
            currentTab = status;
            document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
            document.querySelector(`[data-status="${status}"]`).classList.add('active');
            renderApprovals();
        }
        
        function renderApprovals() {
            const container = document.getElementById('approvals');
            const filtered = allApprovals.filter(a => a.status === currentTab);
            
            if (filtered.length === 0) {
                container.innerHTML = `<p class="empty">No ${currentTab} approvals</p>`;
                return;
            }
            
            container.innerHTML = filtered.map(a => {
                const icon = a.category_type === 'ServerDiscovery' ? '🌐' :
                             a.category_type === 'EffectApproval' ? '⚡' :
                             a.category_type === 'SynthesisApproval' ? '🛠️' :
                             a.category_type === 'LlmPromptApproval' ? '🤖' :
                             a.category_type === 'SecretRequired' ? '🔑' :
                             a.category_type === 'BudgetExtension' ? '💸' : '📋';
                
                const title = a.details.server_name || a.details.capability_id || a.category_type;
                const endpoint = a.details.endpoint ? `<p><strong>Endpoint:</strong> ${a.details.endpoint}</p>` : '';
                const domains = a.details.domain_match ? `<p><strong>Domains:</strong> ${a.details.domain_match.join(', ')}</p>` : '';
                const effects = a.details.effects ? `<p><strong>Effects:</strong> ${a.details.effects.join(', ')}</p>` : '';
                const secretType = a.details.secret_type ? `<p><strong>Type:</strong> ${a.details.secret_type}</p>` : '';
                const description = a.details.description ? `<p><strong>Description:</strong> ${a.details.description}</p>` : '';
                const budgetDetails = a.category_type === 'BudgetExtension' ? `
                    <p><strong>Dimension:</strong> ${a.details.dimension}</p>
                    <p><strong>Requested:</strong> ${a.details.requested_additional}</p>
                    <p><strong>Consumed / Limit:</strong> ${a.details.consumed} / ${a.details.limit}</p>
                    <p><strong>Plan:</strong> ${a.details.plan_id}</p>
                    <p><strong>Intent:</strong> ${a.details.intent_id}</p>
                ` : '';
                
                const secretInputs = a.category_type === 'SecretRequired' ? `
                    <div class="secret-input">
                        <label>Secret Value:</label>
                        <input type="password" id="secret-${a.id}" placeholder="Enter secret value (optional)...">
                    </div>
                    <div class="secret-input">
                        <label>Alternative Environment Variable Name:</label>
                        <input type="text" id="env-var-${a.id}" placeholder="e.g. MY_CUSTOM_KEY (optional)...">
                    </div>
                ` : '';
                
                const actions = currentTab === 'pending' ? `
                    <button class="approve" onclick="approve('${a.id}')">✅ Approve</button>
                    <button class="reject" onclick="reject('${a.id}')">❌ Reject</button>
                ` : `
                    <button class="reapprove" onclick="reapprove('${a.id}')">🔄 Re-approve</button>
                `;
                
                return `
                    <div class="approval-card" data-id="${a.id}">
                        <h3>${icon} ${title} <span class="status-badge status-${a.status}">${a.status}</span></h3>
                        ${endpoint}${domains}${effects}${secretType}${description}${budgetDetails}
                        <p style="font-size: 12px; color: #6e7681;">Requested: ${new Date(a.requested_at).toLocaleString()} | Expires: ${new Date(a.expires_at).toLocaleString()}</p>
                        ${secretInputs}
                        <div class="actions">${actions}</div>
                    </div>
                `;
            }).join('');
        }

        function renderBudgetPanel() {
            const panel = document.getElementById('budget-panel');
            const budgets = allApprovals.filter(a => a.category_type === 'BudgetExtension');
            if (budgets.length === 0) {
                panel.innerHTML = `
                    <h2>Budget Extensions</h2>
                    <p class="empty">No budget approvals yet.</p>
                `;
                return;
            }

            const pending = budgets.filter(a => a.status === 'pending').length;
            const approved = budgets.filter(a => a.status === 'approved').length;
            const rejected = budgets.filter(a => a.status === 'rejected').length;
            const expired = budgets.filter(a => a.status === 'expired').length;

            const items = budgets.slice(0, 6).map(a => {
                const statusClass = `status-${a.status}`;
                return `
                    <div class="budget-item">
                        <div><strong>${a.details.dimension}</strong> <span class="status-tag ${statusClass}">${a.status}</span></div>
                        <small>Requested ${a.details.requested_additional} • ${a.details.consumed}/${a.details.limit}</small>
                        <small>Plan ${a.details.plan_id} • Intent ${a.details.intent_id}</small>
                    </div>
                `;
            }).join('');

            panel.innerHTML = `
                <h2>Budget Extensions</h2>
                <div class="panel-counts">
                    <span>Pending ${pending}</span>
                    <span>Approved ${approved}</span>
                    <span>Rejected ${rejected}</span>
                    <span>Expired ${expired}</span>
                </div>
                ${items}
                ${budgets.length > 6 ? `<small>Showing 6 of ${budgets.length} requests</small>` : ''}
            `;
        }
        
        async function loadApprovals() {
            try {
                const res = await fetch('/api/approvals');
                const data = await res.json();
                allApprovals = data.approvals || [];
                
                // Update counts
                const pending = allApprovals.filter(a => a.status === 'pending').length;
                const rejected = allApprovals.filter(a => a.status === 'rejected').length;
                const expired = allApprovals.filter(a => a.status === 'expired').length;
                document.getElementById('count-pending').textContent = pending;
                document.getElementById('count-rejected').textContent = rejected;
                document.getElementById('count-expired').textContent = expired;
                
                renderApprovals();
                renderBudgetPanel();
            } catch (e) {
                console.error('Failed to load approvals:', e);
            }
        }
        
        async function approve(id) {
            const card = document.querySelector(`[data-id="${id}"]`);
            const secretInput = card?.querySelector(`#secret-${id}`);
            const envVarInput = card?.querySelector(`#env-var-${id}`);
            const body = { reason: "Approved via web UI" };
            if (secretInput?.value) body.secret_value = secretInput.value;
            if (envVarInput?.value) body.provided_env_var = envVarInput.value;
            
            try {
                const res = await fetch(`/api/approvals/${id}/approve`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(body)
                });
                if (res.ok) {
                    card.innerHTML = '<p class="success">✅ Approved successfully!</p>';
                    setTimeout(() => loadApprovals(), 1500);
                } else {
                    const err = await res.json().catch(() => ({}));
                    alert('Error: ' + (err.error || 'Unknown error'));
                }
            } catch(e) {
                alert('Request failed: ' + e);
            }
        }
        
        async function reject(id) {
            const reason = prompt('Enter rejection reason:');
            if (!reason) return;
            const card = document.querySelector(`[data-id="${id}"]`);
            try {
                const res = await fetch(`/api/approvals/${id}/reject`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ reason })
                });
                if (res.ok) {
                    card.innerHTML = '<p class="error">❌ Rejected</p>';
                    setTimeout(() => loadApprovals(), 1500);
                } else {
                    const err = await res.json().catch(() => ({}));
                    alert('Error: ' + (err.error || 'Unknown error'));
                }
            } catch(e) {
                alert('Request failed: ' + e);
            }
        }
        
        async function reapprove(id) {
            const card = document.querySelector(`[data-id="${id}"]`);
            try {
                const res = await fetch(`/api/approvals/${id}/reapprove`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' }
                });
                if (res.ok) {
                    card.innerHTML = '<p class="success">🔄 Re-approved successfully!</p>';
                    setTimeout(() => loadApprovals(), 1500);
                } else {
                    const err = await res.json().catch(() => ({}));
                    alert('Error: ' + (err.error || 'Unknown error'));
                }
            } catch(e) {
                alert('Request failed: ' + e);
            }
        }
        
        // Initial load
        loadApprovals();
        
        // Auto-refresh every 5 seconds
        setInterval(loadApprovals, 5000);
    </script>
</body>
</html>"#.to_string()
}
