use axum::{Router, routing::{get, post}, extract::ws::{Message, WebSocket, WebSocketUpgrade}, response::IntoResponse, extract::State, body::Body, Json};
use tokio::net::TcpListener;
use std::path::PathBuf;
use std::env;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use futures_util::{StreamExt, sink::SinkExt};
use axum::extract::Path;
use axum::http::StatusCode;

// Add CCOS integration
use rtfs_compiler::ccos::runtime_service::{self, RuntimeEvent, RuntimeCommand};

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", content = "data")]
enum ViewerEvent {
    FullUpdate {
        nodes: Vec<serde_json::Value>,
        edges: Vec<serde_json::Value>,
        rtfs_code: String,
    },
    NodeStatusChange {
        id: String,
        status: String,
    },
    StepLog {
        step: String,  // e.g., "GraphGeneration", "PlanGeneration", "Execution"
        status: String,  // "started", "completed", "error"
        message: String,
        details: Option<serde_json::Value>,  // e.g., {intent_id: "..", plan_body: ".."}
    },
    GraphGenerated {
        root_id: String,
        nodes: Vec<serde_json::Value>,
        edges: Vec<serde_json::Value>,
    },
    PlanGenerated {
        intent_id: String,
        plan_id: String,
        rtfs_code: String,
    },
    ReadyForNext {
        next_step: String,  // e.g., "PlanGeneration", "Execution"
    },
}

struct AppState {
    tx: broadcast::Sender<ViewerEvent>,
    // command sender for submitting new goals to the runtime service (optional)
    cmd_tx: Mutex<Option<mpsc::Sender<RuntimeCommand>>>,
    current_graph_id: Mutex<Option<String>>,  // Root intent ID from graph generation
}

#[tokio::main]
async fn main() {
    // Minimal server mode: do not initialize CCOS/runtime here. The state contains an optional
    // command sender that can be populated if a runtime is started externally.
    let (tx, _) = broadcast::channel(100);
    let state = Arc::new(AppState { 
        tx, 
        cmd_tx: Mutex::new(None),
        current_graph_id: Mutex::new(None),
    });

    // Serve the frontend directory via a small static handler and add phased POST endpoints
    let app = Router::new()
        .route("/ws", get(|ws: WebSocketUpgrade, State(state): State<Arc<AppState>>| async move {
            ws.on_upgrade(move |socket| websocket(socket, state.clone()))
        }))
        // Placeholder POST endpoints (not implemented in minimal server build)
        .route("/generate-graph", post(|| async { (StatusCode::NOT_IMPLEMENTED, "Not implemented") }))
        .route("/generate-plans", post(|| async { (StatusCode::NOT_IMPLEMENTED, "Not implemented") }))
        .route("/execute", post(|| async { (StatusCode::NOT_IMPLEMENTED, "Not implemented") }))
        .route("/", get(|| async { serve_file_path(frontend_base().join("index.html"), "text/html; charset=utf-8").await }))
        .route("/*file", get(static_handler))
        .with_state(state);

    let (listener, bound_addr) = bind_with_port_fallback().await.expect("failed to bind any port");
    println!("viewer_server listening on http://{}", bound_addr);
    
    // Run the server normally (minimal mode)
    axum::serve(listener, app).await.expect("server error");
}

async fn static_handler(Path(file): Path<String>) -> impl IntoResponse {
    // Map the wildcard path to the frontend folder
    let base = frontend_base();
    // strip leading slash if present
    let rel = file.trim_start_matches('/');
    let path = base.join(rel);
    // fallback to index if directory or not found
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let content_type = if path.extension().and_then(|s| s.to_str()) == Some("js") {
                "application/javascript; charset=utf-8"
            } else if path.extension().and_then(|s| s.to_str()) == Some("css") {
                "text/css; charset=utf-8"
            } else if path.extension().and_then(|s| s.to_str()) == Some("html") {
                "text/html; charset=utf-8"
            } else if path.extension().and_then(|s| s.to_str()) == Some("png") {
                "image/png"
            } else {
                "application/octet-stream"
            };
            axum::response::Response::builder()
                .status(axum::http::StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => {
            // try to serve index.html as SPA fallback
            serve_file_path(frontend_base().join("index.html"), "text/html; charset=utf-8").await
        }
    }
}

async fn websocket(ws: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = ws.split();
    let mut rx = state.tx.subscribe();

    // run a single select loop instead of spawning tasks to avoid join-handle
    // ownership/abort issues
    loop {
        tokio::select! {
            biased;
            maybe = rx.recv() => {
                match maybe {
                    Ok(event) => {
                        println!("Sending event to WebSocket client: {:?}", event);
                        let json = serde_json::to_string(&event).unwrap();
                        if sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

fn frontend_base() -> PathBuf {
    // viewer_server crate root -> ../rtfs_compiler/src/viewer/web
    PathBuf::from("../rtfs_compiler/src/viewer/web")
}

// index/js/css handlers replaced by `static_handler` and `serve_file_path` helper

async fn serve_file_path(path: PathBuf, content_type: &str) -> axum::response::Response<Body> {
    match tokio::fs::read(&path).await {
        Ok(bytes) => axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(axum::http::header::CONTENT_TYPE, content_type)
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => axum::response::Response::builder()
            .status(axum::http::StatusCode::NOT_FOUND)
            .body(Body::from(format!("Not found: {}", path.display())))
            .unwrap(),
    }
}

#[derive(serde::Deserialize)]
struct GoalPayload {
    goal: String,
}

async fn generate_graph_handler(_state: State<Arc<AppState>>, Json(_payload): Json<GoalPayload>) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Not implemented").into_response()
}

async fn generate_plans_handler(_state: State<Arc<AppState>>) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Not implemented").into_response()
}

async fn execute_handler(_state: State<Arc<AppState>>) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Not implemented").into_response()
}

async fn bind_with_port_fallback() -> std::io::Result<(TcpListener, std::net::SocketAddr)> {
    let start_port: u16 = env::var("VIEWER_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(3001);
    for port in start_port..start_port+10 {
        let addr = std::net::SocketAddr::from(([127,0,0,1], port));
        match TcpListener::bind(addr).await {
            Ok(listener) => return Ok((listener, addr)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "all fallback ports in use"))
}

// Convert CCOS RuntimeEvent to ViewerEvent for the web interface
fn convert_runtime_event_to_viewer(event: RuntimeEvent) -> ViewerEvent {
    match event {
        RuntimeEvent::Started { intent_id, goal } => {
            // Create a simple graph with the intent as a node
            let nodes = vec![
                serde_json::json!({"id": intent_id.clone(), "label": format!("Intent: {}", goal)})
            ];
            let edges = vec![];
            let rtfs_code = format!("(intent \"{}\")", goal);
            ViewerEvent::FullUpdate { nodes, edges, rtfs_code }
        }
        RuntimeEvent::Status { intent_id, status } => {
            ViewerEvent::NodeStatusChange { id: intent_id, status }
        }
        RuntimeEvent::Step { intent_id, desc } => {
            // For now, just update the status with the step description
            ViewerEvent::NodeStatusChange { id: intent_id, status: desc }
        }
        RuntimeEvent::Result { intent_id, result } => {
            // Update the node with the result
            ViewerEvent::NodeStatusChange { id: intent_id, status: format!("Result: {}", result) }
        }
        RuntimeEvent::Error { message } => {
            // Create an error node
            let nodes = vec![
                serde_json::json!({"id": "error", "label": format!("Error: {}", message)})
            ];
            let edges = vec![];
            let rtfs_code = format!("(error \"{}\")", message);
            ViewerEvent::FullUpdate { nodes, edges, rtfs_code }
        }
        RuntimeEvent::Heartbeat => {
            // For heartbeat, we could update a status node or just ignore
            ViewerEvent::NodeStatusChange { id: "system".to_string(), status: "Alive".to_string() }
        }
        RuntimeEvent::Stopped => {
            ViewerEvent::NodeStatusChange { id: "system".to_string(), status: "Stopped".to_string() }
        }
    }
}
