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
use rtfs_compiler::ccos::{CCOS, runtime_service::{self, RuntimeEvent, RuntimeCommand}};
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use std::collections::HashSet;

// Additional imports for arbiter and orchestration
use rtfs_compiler::ccos::arbiter::delegating_arbiter::DelegatingArbiter;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::ccos::types::{Intent, Plan, PlanBody};
use tokio::task::LocalSet;
use std::time::SystemTime;
use std::collections::HashMap;

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
}

struct AppState {
    tx: broadcast::Sender<ViewerEvent>,
    // command sender for submitting new goals to the runtime service
    cmd_tx: Mutex<Option<mpsc::Sender<RuntimeCommand>>>,
}

#[tokio::main]
async fn main() {
    let (tx, _) = broadcast::channel(100);
    let state = Arc::new(AppState { tx, cmd_tx: Mutex::new(None) });

    // Initialize CCOS and runtime service within a LocalSet that persists for the server lifetime
    let local_set = tokio::task::LocalSet::new();
    local_set.run_until(async {
    let ccos = Arc::new(CCOS::new().await.expect("Failed to initialize CCOS"));
    let handle = runtime_service::start_service(Arc::clone(&ccos)).await;
        
    // Send a test goal to demonstrate real CCOS events
    let cmd_tx = handle.commands();
    // store sender in shared state so HTTP handlers can submit goals
    *state.cmd_tx.lock().await = Some(cmd_tx.clone());
        let test_goal = "echo hello world";
        let context = rtfs_compiler::runtime::security::RuntimeContext {
            security_level: rtfs_compiler::runtime::security::SecurityLevel::Controlled,
            allowed_capabilities: std::collections::HashSet::from([
                "ccos.echo".to_string(),
                "ccos.math.add".to_string(),
            ]),
            use_microvm: false,
            max_execution_time: Some(1000),
            max_memory_usage: Some(16777216),
            log_capability_calls: true,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: std::collections::HashSet::new(),
            exposed_context_prefixes: vec![],
            exposed_context_tags: std::collections::HashSet::new(),
            microvm_config_override: None,
            cross_plan_params: std::collections::HashMap::new(),
        };

        println!("Sending test goal: {}", test_goal);
        let _ = cmd_tx.send(rtfs_compiler::ccos::runtime_service::RuntimeCommand::Start {
            goal: test_goal.to_string(),
            context,
        }).await;
        
        let mut evt_rx = handle.subscribe();
        let state_clone = Arc::clone(&state);
        
        // Spawn task to forward CCOS events to WebSocket clients
        tokio::task::spawn_local(async move {
            while let Ok(runtime_event) = evt_rx.recv().await {
                println!("Received runtime event: {:?}", runtime_event);
                let viewer_event = convert_runtime_event_to_viewer(runtime_event);
                println!("Converted to viewer event: {:?}", viewer_event);
                let _ = state_clone.tx.send(viewer_event);
            }
        });
    }).await;

    // Serve the frontend directory via a small static handler and add POST /intent
    let app = Router::new()
        .route("/ws", get(|ws: WebSocketUpgrade, State(state): State<Arc<AppState>>| async move {
            ws.on_upgrade(move |socket| websocket(socket, state.clone()))
        }))
    .route("/intent", post(intent_handler))
    .route("/", get(|| async { serve_file_path(frontend_base().join("index.html"), "text/html; charset=utf-8").await }))
        .route("/*file", get(static_handler))
        .with_state(state);

    let (listener, bound_addr) = bind_with_port_fallback().await.expect("failed to bind any port");
    println!("viewer_server listening on http://{}", bound_addr);
    
    // Run the server within the LocalSet to keep CCOS runtime alive
    local_set.run_until(async {
        axum::serve(listener, app).await.expect("server error");
    }).await;
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
struct IntentPayload {
    goal: String,
}

async fn intent_handler(State(state): State<Arc<AppState>>, Json(payload): Json<IntentPayload>) -> impl IntoResponse {
    // Try to get the command sender
    let guard = state.cmd_tx.lock().await;
    if let Some(sender) = &*guard {
        // Build a default RuntimeContext similar to the test goal
        let context = RuntimeContext {
            security_level: SecurityLevel::Controlled,
            allowed_capabilities: HashSet::from(["ccos.echo".to_string(), "ccos.math.add".to_string()]),
            use_microvm: false,
            max_execution_time: Some(1000),
            max_memory_usage: Some(16777216),
            log_capability_calls: true,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: HashSet::new(),
            exposed_context_prefixes: vec![],
            exposed_context_tags: HashSet::new(),
            microvm_config_override: None,
            cross_plan_params: std::collections::HashMap::new(),
        };

        let cmd = RuntimeCommand::Start { goal: payload.goal.clone(), context };
        // Fire-and-forget the send; report immediate success/failure
        match sender.clone().try_send(cmd) {
            Ok(_) => (StatusCode::ACCEPTED, format!("Goal submitted: {}", payload.goal)),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to send command: {}", e)),
        }
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "Runtime service not ready".to_string())
    }
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
