use axum::{Router, routing::{get, post}, extract::ws::{Message, WebSocket, WebSocketUpgrade}, response::IntoResponse, body::Body, Json};
use tokio::net::TcpListener;
use std::env;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot};
use std::thread;
use axum::extract::{Path, State};
use std::path::PathBuf;
use futures_util::{StreamExt, sink::SinkExt};
use chrono;

// CCOS runtime types for background arbiter calls
use rtfs_compiler::ccos::{CCOS, runtime_service};
use rtfs_compiler::ccos::types::IntentId;
use rtfs_compiler::ccos::arbiter::arbiter_engine::ArbiterEngine;

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

// Request sent from HTTP handler into the CCOS local runtime thread
struct GraphRequest {
    goal: String,
    resp: oneshot::Sender<Result<(String, Vec<serde_json::Value>, Vec<serde_json::Value>), String>>,
}

struct AppState {
    tx: broadcast::Sender<ViewerEvent>,
    // Channel to send graph generation requests to the CCOS-local worker
    graph_req_tx: mpsc::Sender<GraphRequest>,
}

#[derive(serde::Deserialize)]
struct GenerateGraphRequest {
    goal: String,
}

#[derive(serde::Deserialize)]
struct GeneratePlansRequest {
    graph_id: String,
}

#[derive(serde::Deserialize)]
struct ExecuteRequest {
    plan_id: String,
}

#[derive(serde::Serialize)]
struct GenerateGraphResponse {
    success: bool,
    graph: Option<String>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct GeneratePlansResponse {
    success: bool,
    plans: Vec<serde_json::Value>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct ExecuteResponse {
    success: bool,
    result: Option<String>,
    error: Option<String>,
}

async fn generate_graph_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateGraphRequest>,
) -> Json<GenerateGraphResponse> {
    let goal = payload.goal.trim().to_string();
    
    if goal.is_empty() {
        return Json(GenerateGraphResponse {
            success: false,
            graph: None,
            error: Some("Goal cannot be empty".to_string()),
        });
    }

    // Broadcast that graph generation has started
    let _ = state.tx.send(ViewerEvent::StepLog {
        step: "GraphGeneration".to_string(),
        status: "started".to_string(),
        message: format!("Generating graph for goal: {}", goal),
        details: Some(serde_json::json!({
            "goal": goal.clone(),
            "timestamp": chrono::Utc::now().timestamp()
        })),
    });
    // Send request to background CCOS worker which runs in a LocalSet/current-thread runtime
    let (resp_tx, resp_rx) = oneshot::channel();
    let req = GraphRequest { goal: goal.clone(), resp: resp_tx };

    if let Err(_e) = state.graph_req_tx.clone().try_send(req) {
        // channel full or closed - fall back to mock response
        let graph_id = format!("graph_{}", chrono::Utc::now().timestamp());
        let mock_nodes = vec![
            serde_json::json!({"id": "root", "label": format!("Goal: {}", goal), "type": "intent", "status": "active"}),
        ];
        let mock_edges = vec![];
        let _ = state.tx.send(ViewerEvent::GraphGenerated { root_id: graph_id.clone(), nodes: mock_nodes.clone(), edges: mock_edges.clone() });
        let _ = state.tx.send(ViewerEvent::StepLog { step: "GraphGeneration".to_string(), status: "completed".to_string(), message: "Graph generation (fallback) completed".to_string(), details: None });
        return Json(GenerateGraphResponse { success: true, graph: Some(graph_id), error: None });
    }

    // Await response with a timeout to avoid hanging the HTTP request
    match tokio::time::timeout(std::time::Duration::from_secs(30), resp_rx).await {
        Ok(Ok(Ok((root_id, nodes, edges)))) => {
            // Broadcast the generated graph
            let _ = state.tx.send(ViewerEvent::GraphGenerated { root_id: root_id.clone(), nodes: nodes.clone(), edges: edges.clone() });
            // Broadcast completion
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "GraphGeneration".to_string(),
                status: "completed".to_string(),
                message: format!("Graph generated successfully with {} nodes", nodes.len()),
                details: Some(serde_json::json!({
                    "graph_id": root_id.clone(),
                    "node_count": nodes.len(),
                    "edge_count": edges.len()
                })),
            });
            Json(GenerateGraphResponse { success: true, graph: Some(root_id), error: None })
        }
        _ => {
            // Timed out or error - fallback to simple mock
            let graph_id = format!("graph_{}", chrono::Utc::now().timestamp());
            let mock_nodes = vec![serde_json::json!({"id": "root", "label": format!("Goal: {}", goal), "type": "intent", "status": "active"})];
            let mock_edges = vec![];
            let _ = state.tx.send(ViewerEvent::GraphGenerated { root_id: graph_id.clone(), nodes: mock_nodes.clone(), edges: mock_edges.clone() });
            let _ = state.tx.send(ViewerEvent::StepLog { step: "GraphGeneration".to_string(), status: "completed".to_string(), message: "Graph generation (timeout/fallback) completed".to_string(), details: None });
            Json(GenerateGraphResponse { success: true, graph: Some(graph_id), error: Some("CCOS generation timed out or failed, used fallback".to_string()) })
        }
    }
}

async fn generate_plans_handler(
    Json(payload): Json<GeneratePlansRequest>,
) -> Json<GeneratePlansResponse> {
    // Mock implementation for now - use the graph_id from payload
    let _graph_id = payload.graph_id;
    Json(GeneratePlansResponse {
        success: true,
        plans: vec![serde_json::json!({"id": "mock_plan", "code": "(println \"Mock plan\")"})],
        error: None,
    })
}

async fn execute_handler(
    Json(payload): Json<ExecuteRequest>,
) -> Json<ExecuteResponse> {
    // Mock implementation for now - use the plan_id from payload
    let _plan_id = payload.plan_id;
    Json(ExecuteResponse {
        success: true,
        result: Some("Mock execution completed".to_string()),
        error: None,
    })
}

async fn websocket(ws: WebSocket, state: Arc<AppState>) {
    println!("WebSocket connection established!");
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
    println!("WebSocket connection closed!");
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

#[tokio::main]
async fn main() {
    // Initialize CCOS with debug callback
    // Temporarily removed debug callback due to CCOS integration being disabled
    // let debug_callback = Arc::new(|msg: String| {
    //     // Log debug messages
    //     println!("CCOS Debug: {}", msg);
    // });

    // Temporarily removed CCOS initialization due to thread safety issues
    // let ccos = Arc::new(CCOS::new_with_debug_callback(Some(debug_callback)).await.expect("Failed to initialize CCOS"));
    // let handle = runtime_service::start_service(Arc::clone(&ccos)).await;

    let (tx, _) = broadcast::channel(100);

    // Channel for graph generation requests to the CCOS worker
    let (graph_req_tx, graph_req_rx) = mpsc::channel::<GraphRequest>(16);

    // Spawn a dedicated thread that runs a current-thread Tokio runtime + LocalSet
    // This mirrors the example's pattern so we can call non-Send LLM-backed arbiter methods.
    let _worker_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("worker runtime");
        let local = tokio::task::LocalSet::new();

        local.block_on(&rt, async move {
            // Initialize CCOS inside the worker thread
            // Use a minimal debug callback that does nothing to avoid printing noise
            let debug_cb = Arc::new(move |_s: String| {});
            let ccos = Arc::new(match CCOS::new_with_debug_callback(Some(debug_cb)).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init CCOS in worker: {}", e);
                    return;
                }
            });

            // Start runtime service so DelegatingArbiter and graph storage are available
            let _handle = runtime_service::start_service(Arc::clone(&ccos)).await;

            // Process incoming graph requests
            let mut rx = graph_req_rx;
            while let Some(req) = rx.recv().await {
                let goal = req.goal.clone();
                // Try to get delegating arbiter
                if let Some(arb) = ccos.get_delegating_arbiter() {
                    match arb.natural_language_to_graph(&goal).await {
                        Ok(root_id) => {
                            // Wait briefly for persistence
                            for _ in 0..10 {
                                if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                                    if graph_lock.get_intent(&root_id).is_some() { break; }
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }

                            // Collect intents into nodes/edges (simple snapshot)
                            let mut nodes: Vec<serde_json::Value> = Vec::new();
                            let mut edges: Vec<serde_json::Value> = Vec::new();
                            if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                                let all = graph_lock.storage.get_all_intents_sync();
                                for st in all.iter() {
                                    let child_ids = graph_lock.get_child_intents(&st.intent_id).into_iter().map(|c| c.intent_id).collect::<Vec<_>>();
                                    nodes.push(serde_json::json!({"id": st.intent_id.clone(), "label": st.name.clone().unwrap_or_else(|| st.goal.clone()), "type": "intent", "status": st.status.clone()}));
                                    for child in child_ids.iter() {
                                        edges.push(serde_json::json!({"source": st.intent_id.clone(), "target": child.clone(), "type": "depends_on"}));
                                    }
                                }
                            }

                            let _ = req.resp.send(Ok((root_id, nodes, edges)));
                            continue;
                        }
                        Err(e) => {
                            let _ = req.resp.send(Err(format!("arbiter error: {}", e)));
                            continue;
                        }
                    }
                }

                // No arbiter available
                let _ = req.resp.send(Err("no delegating arbiter available".to_string()));
            }
        });
    });

    let state = Arc::new(AppState {
        tx,
        graph_req_tx,
    });

    // Serve the frontend directory via a small static handler and add phased POST endpoints
    let app = Router::new()
        .route("/ws", get(|ws: WebSocketUpgrade, State(state): State<Arc<AppState>>| async move {
            println!("WebSocket upgrade request received!");
            ws.on_upgrade(move |socket| websocket(socket, state.clone()))
        }))
        .route("/generate-graph", post(generate_graph_handler))
        .route("/generate-plans", post(generate_plans_handler))
        .route("/execute", post(execute_handler))
        .route("/", get(|| async { serve_file_path(frontend_base().join("index.html"), "text/html; charset=utf-8").await }))
        .route("/*file", get(static_handler))
        .with_state(state);

    let (listener, bound_addr) = bind_with_port_fallback().await.expect("failed to bind any port");
    println!("viewer_server listening on http://{}", bound_addr);

    // Run the server normally
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

fn frontend_base() -> PathBuf {
    // viewer_server crate root -> ../rtfs_compiler/src/viewer/web
    PathBuf::from("../rtfs_compiler/src/viewer/web")
}

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
