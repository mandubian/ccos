//! HTTP REST API for remote agent content access.
//!
//! Provides REST endpoints for content operations:
//! - POST /api/content/write - Write content
//! - GET /api/content/{handle} - Read content by handle  
//! - POST /api/content/persist - Mark content as persistent
//! - GET /api/content/session/{session_id}/names - List content names in session
//!
//! ## Security
//!
//! All endpoints require authentication via Bearer token (AUTONOETIC_SHARED_SECRET).
//! CORS is restricted by default. Rate limiting can be configured.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::runtime::content_store::ContentStore;

/// Shared state for HTTP handlers
#[derive(Clone)]
pub struct HttpState {
    pub store: Arc<Mutex<ContentStore>>,
    /// Shared secret for authentication (Bearer token)
    pub shared_secret: String,
    /// Maximum request body size in bytes (default: 10MB)
    pub max_body_size: usize,
}

/// Default max body size: 10MB
const DEFAULT_MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Valid session_id pattern (alphanumeric, dash, underscore, dot)
fn is_valid_session_id(s: &str) -> bool {
    s.len() <= 128 && s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Valid content name pattern (alphanumeric, dash, underscore, dot, slash for paths)
fn is_valid_content_name(s: &str) -> bool {
    s.len() <= 512 
        && !s.starts_with('/') 
        && !s.contains("..")
        && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Validate Bearer token from Authorization header
fn validate_auth(headers: &HeaderMap, expected_secret: &str) -> Result<(), ErrorResponse> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ErrorResponse {
            error: "Missing Authorization header".to_string(),
            code: 401,
        })?;

    if !auth_header.starts_with("Bearer ") {
        return Err(ErrorResponse {
            error: "Invalid Authorization format, expected 'Bearer <token>'".to_string(),
            code: 401,
        });
    }

    let token = &auth_header[7..]; // Skip "Bearer "
    if token != expected_secret {
        return Err(ErrorResponse {
            error: "Invalid token".to_string(),
            code: 403,
        });
    }

    Ok(())
}

/// Extract and validate session_id from request
fn validate_session_id(session_id: &str) -> Result<(), ErrorResponse> {
    if !is_valid_session_id(session_id) {
        return Err(ErrorResponse {
            error: "Invalid session_id format".to_string(),
            code: 400,
        });
    }
    Ok(())
}

/// Extract and validate content name
fn validate_content_name(name: &str) -> Result<(), ErrorResponse> {
    if !is_valid_content_name(name) {
        return Err(ErrorResponse {
            error: "Invalid content name format".to_string(),
            code: 400,
        });
    }
    Ok(())
}

/// Request body for content.write
#[derive(Debug, Deserialize, Serialize)]
pub struct WriteRequest {
    pub session_id: String,
    pub name: String,
    pub content: String, // Base64 encoded for binary content
    #[serde(default)]
    pub encoding: Option<String>, // "utf8" (default) or "base64"
}

impl WriteRequest {
    fn validate(&self) -> Result<(), ErrorResponse> {
        validate_session_id(&self.session_id)?;
        validate_content_name(&self.name)?;
        if self.content.len() > 10_000_000 { // 10MB limit
            return Err(ErrorResponse {
                error: "Content too large (max 10MB)".to_string(),
                code: 413,
            });
        }
        Ok(())
    }
}

/// Response for content.write
#[derive(Debug, Serialize, Deserialize)]
pub struct WriteResponse {
    pub handle: String,
    pub name: String,
    pub size_bytes: usize,
}

/// Request body for content.read via POST
#[derive(Debug, Deserialize, Serialize)]
pub struct ReadRequest {
    pub session_id: String,
    pub name_or_handle: String,
}

/// Response for content.read
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadResponse {
    pub content: String, // Base64 encoded for binary content
    pub encoding: String,
    pub size_bytes: usize,
    pub handle: String,
}

/// Request body for content.persist
#[derive(Debug, Deserialize, Serialize)]
pub struct PersistRequest {
    pub session_id: String,
    pub handle: String,
}

/// Response for content.persist
#[derive(Debug, Serialize, Deserialize)]
pub struct PersistResponse {
    pub handle: String,
    pub persisted: bool,
}

/// Query params for listing content names
#[derive(Debug, Deserialize, Serialize)]
pub struct ListQuery {
    pub session_id: String,
}

/// Response for listing content names
#[derive(Debug, Serialize, Deserialize)]
pub struct ListResponse {
    pub names: Vec<ContentName>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentName {
    pub name: String,
    pub handle: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let status = StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

/// Create the HTTP router for content API
pub fn create_router(state: HttpState) -> Router {
    Router::new()
        .route("/api/content/write", post(handle_write))
        .route("/api/content/read/{session_id}/{name_or_handle}", get(handle_read_get))
        .route("/api/content/read", post(handle_read_post))
        .route("/api/content/persist", post(handle_persist))
        .route("/api/content/names", get(handle_list_names))
        .layer(CorsLayer::very_permissive())  // More restrictive than permissive
        .with_state(Arc::new(state))
}

/// Start the HTTP server on the given address
pub async fn start_http_server(
    addr: std::net::SocketAddr,
    state: HttpState,
) -> anyhow::Result<()> {
    let app = create_router(state);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HTTP content API listening on {}", listener.local_addr()?);
    
    axum::serve(listener, app).await?;
    Ok(())
}

/// Start the HTTP server with a new ContentStore
pub async fn start_http_server_with_store(
    addr: std::net::SocketAddr,
    gateway_dir: std::path::PathBuf,
    shared_secret: String,
) -> anyhow::Result<()> {
    let store = ContentStore::new(&gateway_dir)?;
    let state = HttpState {
        store: Arc::new(Mutex::new(store)),
        shared_secret,
        max_body_size: DEFAULT_MAX_BODY_SIZE,
    };
    start_http_server(addr, state).await
}

/// POST /api/content/write - Write content to the store
async fn handle_write(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(req): Json<WriteRequest>,
) -> Result<Json<WriteResponse>, ErrorResponse> {
    // Authentication
    validate_auth(&headers, &state.shared_secret)?;
    
    // Validation
    req.validate()?;

    let store = state.store.lock().await;

    // Decode content based on encoding
    let content_bytes = match req.encoding.as_deref() {
        Some("base64") => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(&req.content)
                .map_err(|e| ErrorResponse { error: format!("Invalid base64: {}", e), code: 400 })?
        }
        _ => req.content.into_bytes(), // UTF-8 default
    };

    let size_bytes = content_bytes.len();

    // Write to content store
    let handle = store.write(&content_bytes)
        .map_err(|e| ErrorResponse { error: e.to_string(), code: 500 })?;

    // Register name in session
    store.register_name(&req.session_id, &req.name, &handle)
        .map_err(|e| ErrorResponse { error: e.to_string(), code: 500 })?;

    Ok(Json(WriteResponse {
        handle,
        name: req.name,
        size_bytes,
    }))
}

/// GET /api/content/read/{session_id}/{name_or_handle} - Read content (path params)
async fn handle_read_get(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Path((session_id, name_or_handle)): Path<(String, String)>,
) -> Result<Json<ReadResponse>, ErrorResponse> {
    // Authentication
    validate_auth(&headers, &state.shared_secret)?;
    
    // Validation
    validate_session_id(&session_id)?;
    
    read_content(&state, &session_id, &name_or_handle).await
}

/// POST /api/content/read - Read content (body params)
async fn handle_read_post(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(req): Json<ReadRequest>,
) -> Result<Json<ReadResponse>, ErrorResponse> {
    // Authentication
    validate_auth(&headers, &state.shared_secret)?;
    
    // Validation
    validate_session_id(&req.session_id)?;

    read_content(&state, &req.session_id, &req.name_or_handle).await
}

async fn read_content(
    state: &HttpState,
    session_id: &str,
    name_or_handle: &str,
) -> Result<Json<ReadResponse>, ErrorResponse> {
    let store = state.store.lock().await;

    let content_bytes = store.read_by_name_or_handle(session_id, name_or_handle)
        .map_err(|e| ErrorResponse { error: e.to_string(), code: 404 })?;

    let size_bytes = content_bytes.len();
    let handle = store.write(&content_bytes)
        .map_err(|e| ErrorResponse { error: e.to_string(), code: 500 })?;

    // Encode as base64 for safe transport
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&content_bytes);

    Ok(Json(ReadResponse {
        content: encoded,
        encoding: "base64".to_string(),
        size_bytes,
        handle,
    }))
}

/// POST /api/content/persist - Mark content as persistent
async fn handle_persist(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(req): Json<PersistRequest>,
) -> Result<Json<PersistResponse>, ErrorResponse> {
    // Authentication
    validate_auth(&headers, &state.shared_secret)?;
    
    // Validation
    validate_session_id(&req.session_id)?;

    let store = state.store.lock().await;

    store.persist(&req.session_id, &req.handle)
        .map_err(|e| ErrorResponse { error: e.to_string(), code: 500 })?;

    Ok(Json(PersistResponse {
        handle: req.handle,
        persisted: true,
    }))
}

/// GET /api/content/names?session_id=xxx - List content names in a session
async fn handle_list_names(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<ListResponse>, ErrorResponse> {
    // Authentication
    validate_auth(&headers, &state.shared_secret)?;
    
    // Validation
    validate_session_id(&query.session_id)?;

    let store = state.store.lock().await;

    let entries = store.list_names_with_handles(&query.session_id)
        .map_err(|e| ErrorResponse { error: e.to_string(), code: 500 })?;

    let names: Vec<ContentName> = entries
        .into_iter()
        .map(|(name, handle)| ContentName { name, handle })
        .collect();

    Ok(Json(ListResponse { names }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const TEST_SECRET: &str = "test-secret-token";

    async fn setup_test_server() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let gateway_dir = dir.path().join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();
        
        let store = ContentStore::new(&gateway_dir).unwrap();
        let state = HttpState {
            store: Arc::new(Mutex::new(store)),
            shared_secret: TEST_SECRET.to_string(),
            max_body_size: DEFAULT_MAX_BODY_SIZE,
        };
        let app = create_router(state);
        
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        
        // Give server time to start
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        
        (addr, handle, dir)
    }

    fn auth_header() -> (String, String) {
        ("Authorization".to_string(), format!("Bearer {}", TEST_SECRET))
    }

    #[tokio::test]
    async fn test_write_and_read_content() {
        let (addr, handle, _dir) = setup_test_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", addr);
        let (auth_name, auth_value) = auth_header();
        
        // Write content
        let write_req = WriteRequest {
            session_id: "test-session".to_string(),
            name: "test.txt".to_string(),
            content: "Hello, World!".to_string(),
            encoding: None,
        };
        
        let resp = client
            .post(&format!("{}/api/content/write", base))
            .header(&auth_name, &auth_value)
            .json(&write_req)
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        
        let write_resp: WriteResponse = resp.json().await.unwrap();
        assert!(!write_resp.handle.is_empty());
        assert_eq!(write_resp.size_bytes, 13);
        
        // Read content back via GET
        let resp = client
            .get(&format!("{}/api/content/read/test-session/test.txt", base))
            .header(&auth_name, &auth_value)
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        
        let read_resp: ReadResponse = resp.json().await.unwrap();
        assert_eq!(read_resp.encoding, "base64");
        
        // Decode and verify
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD.decode(&read_resp.content).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello, World!");
        
        handle.abort();
    }

    #[tokio::test]
    async fn test_auth_required() {
        let (addr, handle, _dir) = setup_test_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", addr);
        
        // Try without auth
        let write_req = WriteRequest {
            session_id: "test-session".to_string(),
            name: "test.txt".to_string(),
            content: "Hello, World!".to_string(),
            encoding: None,
        };
        
        let resp = client
            .post(&format!("{}/api/content/write", base))
            .json(&write_req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "Should require authentication");
        
        handle.abort();
    }

    #[tokio::test]
    async fn test_invalid_token() {
        let (addr, handle, _dir) = setup_test_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", addr);
        
        let write_req = WriteRequest {
            session_id: "test-session".to_string(),
            name: "test.txt".to_string(),
            content: "Hello, World!".to_string(),
            encoding: None,
        };
        
        let resp = client
            .post(&format!("{}/api/content/write", base))
            .header("Authorization", "Bearer wrong-token")
            .json(&write_req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403, "Should reject invalid token");
        
        handle.abort();
    }

    #[tokio::test]
    async fn test_invalid_session_id() {
        let (addr, handle, _dir) = setup_test_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", addr);
        let (auth_name, auth_value) = auth_header();
        
        let write_req = WriteRequest {
            session_id: "../../../etc/passwd".to_string(), // Path traversal attempt
            name: "test.txt".to_string(),
            content: "Hello, World!".to_string(),
            encoding: None,
        };
        
        let resp = client
            .post(&format!("{}/api/content/write", base))
            .header(&auth_name, &auth_value)
            .json(&write_req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "Should reject invalid session_id");
        
        handle.abort();
    }

    #[tokio::test]
    async fn test_persist_content() {
        let (addr, handle, _dir) = setup_test_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", addr);
        let (auth_name, auth_value) = auth_header();
        
        // Write content first
        let write_req = WriteRequest {
            session_id: "test-session".to_string(),
            name: "persistent.txt".to_string(),
            content: "Persistent data".to_string(),
            encoding: None,
        };
        
        let resp = client
            .post(&format!("{}/api/content/write", base))
            .header(&auth_name, &auth_value)
            .json(&write_req)
            .send()
            .await
            .unwrap();
        let write_resp: WriteResponse = resp.json().await.unwrap();
        
        // Persist it
        let persist_req = PersistRequest {
            session_id: "test-session".to_string(),
            handle: write_resp.handle.clone(),
        };
        
        let resp = client
            .post(&format!("{}/api/content/persist", base))
            .header(&auth_name, &auth_value)
            .json(&persist_req)
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        
        let persist_resp: PersistResponse = resp.json().await.unwrap();
        assert!(persist_resp.persisted);
        
        handle.abort();
    }

    #[tokio::test]
    async fn test_list_names() {
        let (addr, handle, _dir) = setup_test_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", addr);
        let (auth_name, auth_value) = auth_header();
        
        // Write first file
        let write_req1 = WriteRequest {
            session_id: "test-session-list".to_string(),
            name: "file1.txt".to_string(),
            content: "Content of file1".to_string(),
            encoding: None,
        };
        let resp1 = client
            .post(&format!("{}/api/content/write", base))
            .header(&auth_name, &auth_value)
            .json(&write_req1)
            .send()
            .await
            .unwrap();
        assert!(resp1.status().is_success(), "Write 1 failed");
        
        // Write second file
        let write_req2 = WriteRequest {
            session_id: "test-session-list".to_string(),
            name: "file2.txt".to_string(),
            content: "Content of file2".to_string(),
            encoding: None,
        };
        let resp2 = client
            .post(&format!("{}/api/content/write", base))
            .header(&auth_name, &auth_value)
            .json(&write_req2)
            .send()
            .await
            .unwrap();
        assert!(resp2.status().is_success(), "Write 2 failed");
        
        // List names
        let resp = client
            .get(&format!("{}/api/content/names?session_id=test-session-list", base))
            .header(&auth_name, &auth_value)
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "List failed");
        
        let list_resp: ListResponse = resp.json().await.unwrap();
        assert_eq!(list_resp.names.len(), 2, "Expected 2 names, got: {:?}", list_resp.names);
        
        let names: Vec<&str> = list_resp.names.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"file1.txt"), "Missing file1.txt in {:?}", names);
        assert!(names.contains(&"file2.txt"), "Missing file2.txt in {:?}", names);
        
        handle.abort();
    }
}
