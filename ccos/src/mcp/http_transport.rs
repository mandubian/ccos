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
}

impl HttpTransportState {
    pub fn new(server: MCPServer) -> Self {
        Self {
            server: Arc::new(server),
            sessions: RwLock::new(HashMap::new()),
            broadcasters: RwLock::new(HashMap::new()),
            approval_queue: None,
        }
    }

    pub fn with_approval_queue(mut self, queue: UnifiedApprovalQueue<FileApprovalStorage>) -> Self {
        self.approval_queue = Some(queue);
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
    run_http_transport_with_approvals(server, config, None).await
}

/// Run the MCP server with Streamable HTTP transport and optional approval queue
pub async fn run_http_transport_with_approvals(
    server: MCPServer,
    config: HttpTransportConfig,
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut state = HttpTransportState::new(server);
    if let Some(queue) = approval_queue {
        state = state.with_approval_queue(queue);
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
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    eprintln!("Starting CCOS MCP server on http://{}/mcp", addr);
    eprintln!("  POST /mcp - Send JSON-RPC requests");
    eprintln!("  GET /mcp  - Open SSE stream");
    eprintln!("  DELETE /mcp - Terminate session");
    eprintln!("  GET /approvals - Approval web UI");
    eprintln!("  GET /api/approvals - List pending approvals (JSON)");

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
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
    let stream = unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let data = serde_json::to_string(&msg).unwrap_or_default();
                    let event = Event::default().event("message").data(data);
                    return Some((Ok::<_, Infallible>(event), rx));
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
    });

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

/// GET /api/approvals - JSON list of pending approvals
async fn handle_api_approvals_list(
    State(state): State<Arc<HttpTransportState>>,
) -> impl IntoResponse {
    let pending = if let Some(ref queue) = state.approval_queue {
        queue.list_pending().await.unwrap_or_default()
    } else {
        vec![]
    };

    let approvals: Vec<Value> = pending
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
            };
            json!({
                "id": req.id,
                "category_type": category_type,
                "details": details,
                "requested_at": req.requested_at.to_rfc3339(),
                "expires_at": req.expires_at.to_rfc3339()
            })
        })
        .collect();

    Json(json!({ "approvals": approvals }))
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
    Json(body): Json<ApproveSecretRequest>,
) -> impl IntoResponse {
    if let Some(ref queue) = state.approval_queue {
        // Note: SecretRequired handling was removed as the feature was deprecated

        // Approve the request
        match queue
            .approve(&id, ApprovalAuthority::User("web".to_string()), body.reason)
            .await
        {
            Ok(()) => (StatusCode::OK, Json(json!({ "success": true, "id": id }))),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            ),
        }
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Approval queue not configured" })),
        )
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
    Json(body): Json<RejectRequest>,
) -> impl IntoResponse {
    if let Some(ref queue) = state.approval_queue {
        match queue
            .reject(&id, ApprovalAuthority::User("web".to_string()), body.reason)
            .await
        {
            Ok(()) => (StatusCode::OK, Json(json!({ "success": true, "id": id }))),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            ),
        }
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Approval queue not configured" })),
        )
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

/// Generate simple HTML page for approvals
fn generate_approvals_html(pending: &[ApprovalRequest]) -> String {
    let approvals_html = if pending.is_empty() {
        "<p class='empty'>No pending approvals</p>".to_string()
    } else {
        pending
            .iter()
            .map(|req| {
                let (title, details, has_secret_input) = match &req.category {
                    ApprovalCategory::ServerDiscovery { server_info, domain_match, .. } => {
                        (format!("🌐 Server Discovery: {}", server_info.name),
                         format!("<p><strong>Endpoint:</strong> {}</p><p><strong>Domains:</strong> {}</p>", server_info.endpoint, domain_match.join(", ")),
                         false)
                    }
                    ApprovalCategory::EffectApproval { capability_id, effects, .. } => {
                        (format!("⚡ Effect Approval: {}", capability_id),
                         format!("<p><strong>Effects:</strong> {}</p>", effects.join(", ")),
                         false)
                    }
                    ApprovalCategory::SynthesisApproval { capability_id, is_pure, .. } => {
                        (format!("🛠️ Synthesis: {}", capability_id),
                         format!("<p><strong>Is Pure:</strong> {}</p>", is_pure),
                         false)
                    }
                    ApprovalCategory::LlmPromptApproval { prompt, risk_reasons } => {
                        (format!("🤖 LLM Prompt Review"),
                         format!("<p><strong>Preview:</strong> {}...</p><p><strong>Risks:</strong> {}</p>", 
                                 prompt.chars().take(100).collect::<String>(), risk_reasons.join(", ")),
                         false)
                    }
                    ApprovalCategory::SecretRequired { capability_id, secret_type, description } => {
                        (format!("🔑 Secret Required: {}", capability_id),
                         format!("<p><strong>Type:</strong> {}</p><p><strong>Description:</strong> {}</p>", secret_type, description),
                         true)
                    }
                };

                let secret_input = if has_secret_input {
                    format!(r#"<div class="secret-input">
                        <label>Secret Value:</label>
                        <input type="password" id="secret-{}" placeholder="Enter secret value (optional)...">
                    </div>
                    <div class="secret-input">
                        <label>Alternative Environment Variable Name:</label>
                        <input type="text" id="env-var-{}" placeholder="e.g. MY_CUSTOM_KEY (optional)...">
                    </div>"#, req.id, req.id)
                } else {
                    String::new()
                };

                format!(r#"
                    <div class="approval-card" data-id="{}">
                        <h3>{}</h3>
                        {}
                        {}
                        <div class="actions">
                            <button class="approve" onclick="approve('{}')">✅ Approve</button>
                            <button class="reject" onclick="reject('{}')">❌ Reject</button>
                        </div>
                    </div>
                "#, req.id, title, details, secret_input, req.id, req.id)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>CCOS Approvals</title>
    <style>
        body {{ font-family: -apple-system, system-ui, sans-serif; max-width: 800px; margin: 0 auto; padding: 20px; background: #0d1117; color: #c9d1d9; }}
        h1 {{ color: #58a6ff; }}
        .approval-card {{ background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 16px; margin-bottom: 16px; }}
        .approval-card h3 {{ margin-top: 0; color: #f0f6fc; }}
        .approval-card p {{ margin: 8px 0; color: #8b949e; }}
        .secret-input {{ margin: 12px 0; }}
        .secret-input label {{ display: block; margin-bottom: 4px; color: #8b949e; }}
        .secret-input input {{ width: 100%; padding: 8px; border: 1px solid #30363d; border-radius: 4px; background: #0d1117; color: #c9d1d9; }}
        .actions {{ margin-top: 12px; display: flex; gap: 8px; }}
        .actions button {{ padding: 8px 16px; border: none; border-radius: 4px; cursor: pointer; font-weight: 500; }}
        .approve {{ background: #238636; color: white; }}
        .approve:hover {{ background: #2ea043; }}
        .reject {{ background: #da3633; color: white; }}
        .reject:hover {{ background: #f85149; }}
        .empty {{ color: #8b949e; font-style: italic; }}
        .success {{ color: #3fb950; }}
        .error {{ color: #f85149; }}
    </style>
</head>
<body>
    <h1>🔐 CCOS Pending Approvals</h1>
    <div id="approvals">
        {}
    </div>
    <script>
        async function approve(id) {{
            const card = document.querySelector(`[data-id="${{id}}"]`);
            const secretInput = card?.querySelector(`#secret-${{id}}`);
            const envVarInput = card?.querySelector(`#env-var-${{id}}`);
            const body = {{ reason: "Approved via web UI" }};
            if (secretInput?.value) {{
                body.secret_value = secretInput.value;
            }}
            if (envVarInput?.value) {{
                body.provided_env_var = envVarInput.value;
            }}
            try {{
                const res = await fetch(`/api/approvals/${{id}}/approve`, {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify(body)
                }});
                if (res.ok) {{
                    card.innerHTML = '<p class="success">✅ Approved successfully!</p>';
                    setTimeout(() => card.remove(), 2000);
                }} else {{
                    let errorMsg = 'An unknown error occurred';
                    try {{
                        const err = await res.json();
                        errorMsg = err.error || errorMsg;
                    }} catch (e) {{
                        errorMsg = `Server error (${{res.status}}): ${{res.statusText}}`;
                    }}
                    alert('Error: ' + errorMsg);
                }}
            }} catch(e) {{
                alert('Request failed: ' + e);
            }}
        }}
        async function reject(id) {{
            const reason = prompt('Enter rejection reason:');
            if (!reason) return;
            const card = document.querySelector(`[data-id="${{id}}"]`);
            try {{
                const res = await fetch(`/api/approvals/${{id}}/reject`, {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify({{ reason }})
                }});
                if (res.ok) {{
                    card.innerHTML = '<p class="error">❌ Rejected</p>';
                    setTimeout(() => card.remove(), 2000);
                }} else {{
                    let errorMsg = 'An unknown error occurred';
                    try {{
                        const err = await res.json();
                        errorMsg = err.error || errorMsg;
                    }} catch (e) {{
                        errorMsg = `Server error (${{res.status}}): ${{res.statusText}}`;
                    }}
                    alert('Error: ' + errorMsg);
                }}
            }} catch(e) {{
                alert('Request failed: ' + e);
            }}
        }}
    </script>
</body>
</html>"#,
        approvals_html
    )
}
