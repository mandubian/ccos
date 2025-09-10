use axum::{Router, routing::{get, post}, extract::ws::{Message, WebSocket, WebSocketUpgrade}, response::IntoResponse, body::Body, Json};
use tokio::net::TcpListener;
use std::env;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot};
use axum::extract::{Path, State};
use std::path::PathBuf;
use futures_util::{StreamExt, sink::SinkExt};
use chrono;

// CCOS runtime types for background arbiter calls
use rtfs_compiler::ccos::{CCOS, runtime_service};
use rtfs_compiler::ccos::arbiter::arbiter_engine::ArbiterEngine;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", content = "data")]
#[allow(dead_code)]
enum ViewerEvent {
    FullUpdate {
        nodes: Vec<serde_json::Value>,
        edges: Vec<serde_json::Value>,
        rtfs_code: String,
    },
    NodeStatusChange {
        id: String,
        status: String,
        details: Option<serde_json::Value>,
    },
    StepLog {
        step: String,  // e.g., "GraphGeneration", "PlanGeneration", "Execution"
        status: String,  // "started", "completed", "error"
        message: String,
        details: Option<serde_json::Value>,  // e.g., {intent_id: "..", plan_body: ".."}
    },
    GraphGenerated {
        root_id: String,
        graph_id: String,  // Same as root_id for now, but allows for future multi-graph support
        nodes: Vec<serde_json::Value>,
        edges: Vec<serde_json::Value>,
    },
    PlanGenerated {
        intent_id: String,
        graph_id: String,  // Graph this plan belongs to
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

struct PlanRequest {
    graph_id: String,
    resp: oneshot::Sender<Result<Vec<serde_json::Value>, String>>,
}

struct ExecuteRequestInternal {
    graph_id: String,
    resp: oneshot::Sender<Result<String, String>>,
}

struct LoadGraphRequestInternal {
    nodes: Vec<serde_json::Value>,
    edges: Vec<serde_json::Value>,
    root_id: Option<String>,
    resp: oneshot::Sender<Result<String, String>>,
}

struct GetPlansRequestInternal {
    graph_id: String,
    resp: oneshot::Sender<Result<Vec<serde_json::Value>, String>>,
}

struct AppState {
    tx: broadcast::Sender<ViewerEvent>,
    // Channel to send graph generation requests to the CCOS-local worker
    graph_req_tx: mpsc::Sender<GraphRequest>,
    plan_req_tx: mpsc::Sender<PlanRequest>,
    execute_req_tx: mpsc::Sender<ExecuteRequestInternal>,
    load_graph_req_tx: mpsc::Sender<LoadGraphRequestInternal>,
    get_plans_req_tx: mpsc::Sender<GetPlansRequestInternal>,
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
    graph_id: String,  // Changed from plan_id to graph_id to match the demo pattern
}

#[derive(serde::Deserialize)]
struct LoadGraphRequest {
    nodes: Vec<serde_json::Value>,
    edges: Vec<serde_json::Value>,
    root_id: Option<String>,
}

#[derive(serde::Deserialize)]
struct GetPlansRequest {
    graph_id: String,
}

#[derive(serde::Serialize)]
struct GenerateGraphResponse {
    success: bool,
    graph: Option<String>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct GetPlansResponse {
    success: bool,
    plans: Option<Vec<serde_json::Value>>,
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

#[derive(serde::Serialize)]
struct LoadGraphResponse {
    success: bool,
    graph_id: Option<String>,
    error: Option<String>,
}

async fn generate_graph_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateGraphRequest>,
) -> Json<GenerateGraphResponse> {
    println!("📨 Received generate-graph request");
    let goal = payload.goal.trim().to_string();
    println!("🎯 Goal: \"{}\"", goal);
    
    if goal.is_empty() {
        println!("❌ Goal is empty, rejecting request");
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

    println!("📨 Sending request to worker thread...");
    if let Err(_e) = state.graph_req_tx.clone().try_send(req) {
        println!("❌ Failed to send request to worker thread: {:?}", _e);
        // channel full or closed - fall back to mock response
        let graph_id = format!("graph_{}", chrono::Utc::now().timestamp());
        let mock_nodes = vec![
            serde_json::json!({"id": "root", "label": format!("Goal: {}", goal), "type": "intent", "status": "active"}),
        ];
        let mock_edges = vec![];
        let _ = state.tx.send(ViewerEvent::GraphGenerated { root_id: graph_id.clone(), graph_id: graph_id.clone(), nodes: mock_nodes.clone(), edges: mock_edges.clone() });
        let _ = state.tx.send(ViewerEvent::StepLog { step: "GraphGeneration".to_string(), status: "completed".to_string(), message: "Graph generation (fallback) completed".to_string(), details: None });
        return Json(GenerateGraphResponse { success: true, graph: Some(graph_id), error: None });
    }
    println!("✅ Request successfully sent to worker thread");

    // Await response with a timeout to avoid hanging the HTTP request
    println!("⏳ Awaiting response from worker thread...");
    match tokio::time::timeout(std::time::Duration::from_secs(30), resp_rx).await {
        Ok(Ok(Ok((root_id, nodes, edges)))) => {
            println!("✅ Received successful response: {} nodes, {} edges", nodes.len(), edges.len());
            // Broadcast the generated graph
            let _ = state.tx.send(ViewerEvent::GraphGenerated { root_id: root_id.clone(), graph_id: root_id.clone(), nodes: nodes.clone(), edges: edges.clone() });
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
        Ok(Ok(Err(e))) => {
            println!("❌ Received error response from worker: {}", e);
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "GraphGeneration".to_string(),
                status: "error".to_string(),
                message: format!("Graph generation failed: {}", e),
                details: Some(serde_json::json!({
                    "goal": goal.clone(),
                    "error": e.clone()
                })),
            });
            Json(GenerateGraphResponse {
                success: false,
                graph: None,
                error: Some(e),
            })
        }
        _ => {
            println!("⏰ Request timed out waiting for worker response");
            // Timed out or error - fallback to simple mock
            let graph_id = format!("graph_{}", chrono::Utc::now().timestamp());
            let mock_nodes = vec![serde_json::json!({"id": "root", "label": format!("Goal: {}", goal), "type": "intent", "status": "active"})];
            let mock_edges = vec![];
            let _ = state.tx.send(ViewerEvent::GraphGenerated { root_id: graph_id.clone(), graph_id: graph_id.clone(), nodes: mock_nodes.clone(), edges: mock_edges.clone() });
            let _ = state.tx.send(ViewerEvent::StepLog { step: "GraphGeneration".to_string(), status: "completed".to_string(), message: "Graph generation (timeout/fallback) completed".to_string(), details: None });
            Json(GenerateGraphResponse { success: true, graph: Some(graph_id), error: Some("CCOS generation timed out or failed, used fallback".to_string()) })
        }
    }
}

async fn generate_plans_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GeneratePlansRequest>,
) -> Json<GeneratePlansResponse> {
    println!("📨 Received generate-plans request for graph: {}", payload.graph_id);
    let graph_id = payload.graph_id.clone();

    // Broadcast that plan generation has started
    let _ = state.tx.send(ViewerEvent::StepLog {
        step: "PlanGeneration".to_string(),
        status: "started".to_string(),
        message: format!("Generating plans for graph: {}", graph_id),
        details: Some(serde_json::json!({
            "graph_id": graph_id.clone(),
            "timestamp": chrono::Utc::now().timestamp()
        })),
    });

    // Send request to background CCOS worker
    let (resp_tx, resp_rx) = oneshot::channel();
    let req = PlanRequest { 
        graph_id: graph_id.clone(), 
        resp: resp_tx 
    };

    if let Err(_e) = state.plan_req_tx.clone().try_send(req) {
        return Json(GeneratePlansResponse {
            success: false,
            plans: vec![],
            error: Some("Plan generation service unavailable".to_string()),
        });
    }

    // Await response with timeout
    println!("⏳ Awaiting plan generation response from worker thread...");
    match tokio::time::timeout(std::time::Duration::from_secs(120), resp_rx).await {
        Ok(Ok(Ok(plans))) => {
            println!("✅ Received successful plan generation response: {} plans", plans.len());

            // Broadcast PlanGenerated events for each plan
            for plan in &plans {
                if let (Some(intent_id), Some(plan_id), Some(body)) = (
                    plan.get("intent_id").and_then(|v| v.as_str()),
                    plan.get("plan_id").and_then(|v| v.as_str()),
                    plan.get("body").and_then(|v| v.as_str()),
                ) {
                    println!("📡 Broadcasting PlanGenerated event for plan: {}", plan_id);
                    let _ = state.tx.send(ViewerEvent::PlanGenerated {
                        intent_id: intent_id.to_string(),
                        graph_id: graph_id.clone(),
                        plan_id: plan_id.to_string(),
                        rtfs_code: body.to_string(),
                    });

                    // Also broadcast node status change to show intent has a plan
                    println!("🔄 Broadcasting intent status update for plan availability");
                    let _ = state.tx.send(ViewerEvent::NodeStatusChange {
                        id: intent_id.to_string(),
                        status: "has_plan".to_string(),
                        details: Some(serde_json::json!({
                            "plan_id": plan_id,
                            "plan_body_preview": body.chars().take(100).collect::<String>(),
                            "has_plan": true
                        })),
                    });
                }
            }

            // Broadcast completion
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "PlanGeneration".to_string(),
                status: "completed".to_string(),
                message: format!("Generated {} plans successfully", plans.len()),
                details: Some(serde_json::json!({
                    "graph_id": graph_id.clone(),
                    "plan_count": plans.len()
                })),
            });
            Json(GeneratePlansResponse { success: true, plans, error: None })
        }
        Ok(Ok(Err(e))) => {
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "PlanGeneration".to_string(),
                status: "error".to_string(),
                message: format!("Plan generation failed: {}", e),
                details: Some(serde_json::json!({
                    "graph_id": graph_id.clone(),
                    "error": e.clone()
                })),
            });
    Json(GeneratePlansResponse {
                success: false,
                plans: vec![],
                error: Some(e),
            })
        }
        _ => {
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "PlanGeneration".to_string(),
                status: "error".to_string(),
                message: "Plan generation timed out".to_string(),
                details: Some(serde_json::json!({
                    "graph_id": graph_id.clone(),
                    "error": "timeout"
                })),
            });
    Json(GeneratePlansResponse {
                success: false,
                plans: vec![],
                error: Some("Plan generation timed out".to_string()),
            })
        }
    }
}

async fn execute_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExecuteRequest>,
) -> Json<ExecuteResponse> {
    let graph_id = payload.graph_id.clone();

    // Broadcast that execution has started
    let _ = state.tx.send(ViewerEvent::StepLog {
        step: "Execution".to_string(),
        status: "started".to_string(),
        message: format!("Executing graph: {}", graph_id),
        details: Some(serde_json::json!({
            "graph_id": graph_id.clone(),
            "timestamp": chrono::Utc::now().timestamp()
        })),
    });

    // Send request to background CCOS worker
    let (resp_tx, resp_rx) = oneshot::channel();
    let req = ExecuteRequestInternal { graph_id: graph_id.clone(), resp: resp_tx };

    if let Err(_e) = state.execute_req_tx.clone().try_send(req) {
        return Json(ExecuteResponse {
            success: false,
            result: None,
            error: Some("Execution service unavailable".to_string()),
        });
    }

    // Await response with timeout
    match tokio::time::timeout(std::time::Duration::from_secs(300), resp_rx).await {
        Ok(Ok(Ok(result))) => {
            // Broadcast completion
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "Execution".to_string(),
                status: "completed".to_string(),
                message: format!("Execution completed successfully"),
                details: Some(serde_json::json!({
                    "graph_id": graph_id.clone(),
                    "result": result.clone()
                })),
            });
            Json(ExecuteResponse { success: true, result: Some(result), error: None })
        }
        Ok(Ok(Err(e))) => {
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "Execution".to_string(),
                status: "error".to_string(),
                message: format!("Execution failed: {}", e),
                details: Some(serde_json::json!({
                    "graph_id": graph_id.clone(),
                    "error": e.clone()
                })),
            });
    Json(ExecuteResponse {
                success: false,
                result: None,
                error: Some(e),
            })
        }
        _ => {
            let _ = state.tx.send(ViewerEvent::StepLog {
                step: "Execution".to_string(),
                status: "error".to_string(),
                message: "Execution timed out".to_string(),
                details: Some(serde_json::json!({
                    "graph_id": graph_id.clone(),
                    "error": "timeout"
                })),
            });
    Json(ExecuteResponse {
                success: false,
                result: None,
                error: Some("Execution timed out".to_string()),
            })
        }
    }
}

async fn load_graph_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoadGraphRequest>,
) -> Json<LoadGraphResponse> {
    println!("📨 Received load-graph request with {} nodes and {} edges",
             payload.nodes.len(), payload.edges.len());

    // Send request to background CCOS worker
    let (resp_tx, resp_rx) = oneshot::channel();
    let req = LoadGraphRequestInternal {
        nodes: payload.nodes.clone(),
        edges: payload.edges.clone(),
        root_id: payload.root_id.clone(),
        resp: resp_tx,
    };

    if let Err(_e) = state.load_graph_req_tx.clone().try_send(req) {
        return Json(LoadGraphResponse {
            success: false,
            graph_id: None,
            error: Some("Load graph service unavailable".to_string()),
        });
    }

    // Await response with timeout
    match tokio::time::timeout(std::time::Duration::from_secs(30), resp_rx).await {
        Ok(Ok(Ok(graph_id))) => {
            println!("✅ Successfully loaded graph with ID: {}", graph_id);
            Json(LoadGraphResponse {
        success: true,
                graph_id: Some(graph_id),
        error: None,
    })
        }
        Ok(Ok(Err(e))) => {
            println!("❌ Failed to load graph: {}", e);
            Json(LoadGraphResponse {
                success: false,
                graph_id: None,
                error: Some(e),
            })
        }
        _ => {
            Json(LoadGraphResponse {
                success: false,
                graph_id: None,
                error: Some("Load graph timed out".to_string()),
            })
        }
    }
}

async fn get_plans_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GetPlansRequest>,
) -> Json<GetPlansResponse> {
    println!("📨 Received get-plans request for graph: {}", payload.graph_id);

    // Send request to background CCOS worker
    let (resp_tx, resp_rx) = oneshot::channel();
    let req = GetPlansRequestInternal {
        graph_id: payload.graph_id.clone(),
        resp: resp_tx,
    };

    if let Err(_e) = state.get_plans_req_tx.clone().try_send(req) {
        return Json(GetPlansResponse {
            success: false,
            plans: None,
            error: Some("Get plans service unavailable".to_string()),
        });
    }

    // Await response with timeout
    match tokio::time::timeout(std::time::Duration::from_secs(10), resp_rx).await {
        Ok(Ok(Ok(plans))) => {
            println!("✅ Successfully retrieved {} plans for graph: {}", plans.len(), payload.graph_id);
            Json(GetPlansResponse {
                success: true,
                plans: Some(plans),
                error: None,
            })
        }
        Ok(Ok(Err(e))) => {
            println!("❌ Failed to get plans: {}", e);
            Json(GetPlansResponse {
                success: false,
                plans: None,
                error: Some(e),
            })
        }
        _ => {
            Json(GetPlansResponse {
                success: false,
                plans: None,
                error: Some("Get plans timed out".to_string()),
            })
        }
    }
}

async fn websocket(ws: WebSocket, state: Arc<AppState>) {
    println!("🔌 WebSocket connection established!");
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
                        println!("📤 Sending event to WebSocket client: {:?}", event);
                        let json = serde_json::to_string(&event).unwrap();
                        if sender.send(Message::Text(json)).await.is_err() {
                            println!("❌ Failed to send WebSocket message");
                            break;
                        } else {
                            println!("✅ WebSocket message sent successfully");
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
    println!("🚀 Starting CCOS Viewer Server...");

    // Initialize CCOS with debug callback
    println!("🔧 Initializing CCOS...");

    let debug_callback = Arc::new(move |msg: String| {
        println!("🔍 CCOS Debug: {}", msg);
    });

    println!("📦 Creating CCOS instance with file storage...");
    
    // Create file storage config for demo persistence
    let storage_path = std::path::PathBuf::from("demo_storage");
    let intent_graph_config = rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig::with_file_storage(storage_path.clone());
    let plan_archive_path = storage_path.join("plans");
    
    let ccos = Arc::new(match CCOS::new_with_configs_and_debug_callback(intent_graph_config, Some(plan_archive_path), Some(debug_callback)).await {
        Ok(c) => {
            println!("✅ CCOS initialized successfully with file storage");
            c
        }
        Err(e) => {
            eprintln!("❌ Failed to initialize CCOS: {}", e);
            std::process::exit(1);
        }
    });

    println!("⚙️ Starting CCOS runtime service...");
    // Wrap runtime service initialization in LocalSet since it spawns local tasks
    let ccos_clone = Arc::clone(&ccos);
    let _handle = tokio::task::LocalSet::new().run_until(async move {
        runtime_service::start_service(ccos_clone).await
    }).await;
    println!("✅ CCOS runtime service started");

    println!("🔍 Checking for delegating arbiter...");
    println!("📋 Environment variables:");
    println!("   OPENROUTER_API_KEY: {}", std::env::var("OPENROUTER_API_KEY").map(|_| "SET").unwrap_or("NOT SET"));
    println!("   OPENAI_API_KEY: {}", std::env::var("OPENAI_API_KEY").map(|_| "SET").unwrap_or("NOT SET"));
    println!("   LLM_MODEL: {}", std::env::var("LLM_MODEL").unwrap_or("NOT SET".to_string()));
    println!("   RTFS_LOCAL_MODEL_PATH: {}", std::env::var("RTFS_LOCAL_MODEL_PATH").unwrap_or("NOT SET".to_string()));

    if let Some(_arb) = ccos.get_delegating_arbiter() {
        println!("✅ Delegating arbiter available (LLM integration enabled)");
    } else {
        println!("❌ No delegating arbiter found (LLM integration disabled)");
        println!("   This means graph generation will fail!");
    }

    let (tx, _) = broadcast::channel(100);

    // Channels for requests to the CCOS worker
    let (graph_req_tx, graph_req_rx) = mpsc::channel::<GraphRequest>(16);
    let (plan_req_tx, plan_req_rx) = mpsc::channel::<PlanRequest>(16);
    let (execute_req_tx, execute_req_rx) = mpsc::channel::<ExecuteRequestInternal>(16);
    let (load_graph_req_tx, load_graph_req_rx) = mpsc::channel::<LoadGraphRequestInternal>(16);
    let (get_plans_req_tx, mut get_plans_req_rx) = mpsc::channel::<GetPlansRequestInternal>(16);

    // Spawn a dedicated thread that runs a current-thread Tokio runtime + LocalSet
    // This mirrors the example's pattern so we can call non-Send LLM-backed arbiter methods.
    let _worker_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("worker runtime");
        let local = tokio::task::LocalSet::new();

        local.block_on(&rt, async move {
            // Initialize CCOS inside the worker thread with file storage
            // Use a minimal debug callback that does nothing to avoid printing noise
            let debug_cb = Arc::new(move |_s: String| {});
            
            // Create file storage config for demo persistence (same as main thread)
            let storage_path = std::path::PathBuf::from("demo_storage");
            let intent_graph_config = rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig::with_file_storage(storage_path.clone());
            let plan_archive_path = storage_path.join("plans");
            
            let ccos = Arc::new(match CCOS::new_with_configs_and_debug_callback(intent_graph_config, Some(plan_archive_path), Some(debug_cb)).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init CCOS in worker: {}", e);
                    return;
                }
            });

            // Start runtime service so DelegatingArbiter and graph storage are available
            let _handle = runtime_service::start_service(Arc::clone(&ccos)).await;

            // Process incoming requests using select! to handle multiple channels
            let mut graph_rx = graph_req_rx;
            let mut plan_rx = plan_req_rx;
            let mut execute_rx = execute_req_rx;
            let mut load_graph_rx = load_graph_req_rx;

            loop {
                tokio::select! {
                    Some(req) = graph_rx.recv() => {
                        println!("🔄 Processing graph generation request in worker thread");
                let goal = req.goal.clone();
                        println!("📝 Processing goal: \"{}\"", goal);

                        // Try to get delegating arbiter for graph generation
                        println!("🔍 Checking for delegating arbiter...");
                if let Some(arb) = ccos.get_delegating_arbiter() {
                            println!("✅ Delegating arbiter found, calling natural_language_to_graph...");
                    match arb.natural_language_to_graph(&goal).await {
                        Ok(root_id) => {
                                    println!("🎉 Graph generation successful, root_id: {}", root_id);
                            // Wait briefly for persistence
                                    println!("⏳ Waiting for graph persistence...");
                                    for i in 0..10 {
                                if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                                            if graph_lock.get_intent(&root_id).is_some() {
                                                println!("✅ Graph persisted after {} attempts", i + 1);
                                                break;
                                            }
                                        }
                                        if i == 9 {
                                            println!("⚠️ Graph persistence timeout - proceeding anyway");
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }

                                    // Collect only the connected component starting from root_id
                                    println!("📊 Collecting connected component from root_id: {}", root_id);
                            let mut nodes: Vec<serde_json::Value> = Vec::new();
                            let mut edges: Vec<serde_json::Value> = Vec::new();

                            if let Ok(mut graph_lock) = ccos.get_intent_graph().lock() {
                                        // Collect connected component using BFS from root_id
                                        use std::collections::{HashMap, VecDeque, HashSet};
                                        let mut visited: HashSet<String> = HashSet::new();
                                        let mut queue: VecDeque<String> = VecDeque::new();
                                        let mut connected_intents: Vec<_> = Vec::new();
                                        
                                        queue.push_back(root_id.clone());
                                        visited.insert(root_id.clone());
                                        
                                        while let Some(current_id) = queue.pop_front() {
                                            // Get the current intent
                                            if let Some(intent) = graph_lock.storage.get_intent_sync(&current_id) {
                                                connected_intents.push(intent.clone());
                                                
                                                // Add children to queue
                                                let child_intents = graph_lock.get_child_intents(&current_id);
                                                for child in child_intents {
                                                    if !visited.contains(&child.intent_id) {
                                                        visited.insert(child.intent_id.clone());
                                                        queue.push_back(child.intent_id.clone());
                                                    }
                                                }
                                                
                                                // Add parent to queue if it exists
                                                if let Some(parent_id) = &intent.parent_intent {
                                                    if !visited.contains(parent_id) {
                                                        visited.insert(parent_id.clone());
                                                        queue.push_back(parent_id.clone());
                                                    }
                                                }
                                            }
                                        }
                                        
                                        println!("📋 Found {} intents in connected component", connected_intents.len());

                                        // Add graph_id metadata to all intents in the connected component
                                        println!("🏷️ Adding graph_id metadata to all intents in connected component");
                                        for intent in &connected_intents {
                                            let mut updated_intent = intent.clone();
                                            updated_intent.metadata.insert("graph_id".to_string(), root_id.clone());
                                            // Mark root intent specially
                                            if intent.intent_id == root_id {
                                                updated_intent.name = Some("Root".to_string());
                                            }
                                            match graph_lock.storage.update_intent(&updated_intent).await {
                                                Ok(_) => println!("✅ Added graph_id to intent: {}", intent.intent_id),
                                                Err(e) => println!("⚠️ Failed to update intent {} with graph_id: {}", intent.intent_id, e),
                                            }
                                        }

                                        // Build dependency graph for topological sorting
                                        let mut dependency_graph: HashMap<String, Vec<String>> = HashMap::new();
                                        let mut incoming_edges: HashMap<String, usize> = HashMap::new();
                                        let mut intent_map: HashMap<String, _> = HashMap::new();

                                        // Initialize structures
                                        for intent in &connected_intents {
                                            dependency_graph.insert(intent.intent_id.clone(), Vec::new());
                                            incoming_edges.insert(intent.intent_id.clone(), 0);
                                            intent_map.insert(intent.intent_id.clone(), intent.clone());
                                        }

                                        // Build dependency relationships
                                        for intent in &connected_intents {
                                            // Parent relationships (parent depends on children)
                                            if let Some(parent_id) = &intent.parent_intent {
                                                if dependency_graph.contains_key(parent_id) {
                                                    dependency_graph.get_mut(parent_id).unwrap().push(intent.intent_id.clone());
                                                    *incoming_edges.get_mut(&intent.intent_id).unwrap() += 1;
                                                }
                                            }

                                            // Child relationships (intent depends on children)
                                            let child_intents = graph_lock.get_child_intents(&intent.intent_id);
                                            for child in child_intents {
                                                if dependency_graph.contains_key(&child.intent_id) {
                                                    dependency_graph.get_mut(&intent.intent_id).unwrap().push(child.intent_id.clone());
                                                    *incoming_edges.get_mut(&child.intent_id).unwrap() += 1;
                                                }
                                            }
                                        }

                                        // Topological sort using Kahn's algorithm
                                        let mut queue: VecDeque<String> = VecDeque::new();
                                        let mut sorted_order: Vec<String> = Vec::new();

                                        // Start with nodes that have no incoming edges (root nodes)
                                        for (intent_id, count) in &incoming_edges {
                                            if *count == 0 {
                                                queue.push_back(intent_id.clone());
                                            }
                                        }

                                        while let Some(intent_id) = queue.pop_front() {
                                            sorted_order.push(intent_id.clone());

                                            if let Some(dependents) = dependency_graph.get(&intent_id) {
                                                for dependent in dependents {
                                                    if let Some(count) = incoming_edges.get_mut(dependent) {
                                                        *count -= 1;
                                                        if *count == 0 {
                                                            queue.push_back(dependent.clone());
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // Fallback: add any remaining nodes (handles cycles)
                                        for intent in &connected_intents {
                                            if !sorted_order.contains(&intent.intent_id) {
                                                sorted_order.push(intent.intent_id.clone());
                                            }
                                        }

                                        println!("📋 Topological sort completed: {} nodes ordered", sorted_order.len());

                                        // Identify the root node (the one we started with)
                                        let root_node_id = Some(root_id.clone());

                                        println!("👑 Root node identified: {:?}", root_node_id);
                                        if let Some(root_intent) = intent_map.get(&root_id) {
                                            println!("   Root goal: \"{}\"", root_intent.goal);
                                            println!("   Root has {} children", graph_lock.get_child_intents(&root_id).len());
                                        }

                                        // Debug: Show all intents in connected component
                                        println!("📋 Intents in connected component:");
                                        for intent in &connected_intents {
                                            let parent_info = intent.parent_intent.as_ref()
                                                .map(|p| format!("parent: {}", p))
                                                .unwrap_or_else(|| "ROOT".to_string());
                                            println!("   {}: \"{}\" ({})", intent.intent_id, intent.goal, parent_info);
                                        }

                                        // Create nodes with special handling for root node
                                        let mut execution_index = 1;

                                        println!("🔄 Processing {} intents in sorted order...", sorted_order.len());
                                        for (index, intent_id) in sorted_order.iter().enumerate() {
                                            if let Some(st) = intent_map.get(intent_id.as_str()) {
                                                println!("  [{}] Processing intent: {} (is_root: {})", index + 1, intent_id, root_node_id.as_ref() == Some(intent_id));
                                                let base_label = if let Some(name) = &st.name {
                                                    if name.is_empty() {
                                                        st.goal.clone()
                                                    } else {
                                                        name.clone()
                                                    }
                                                } else {
                                                    st.goal.clone()
                                                };

                                                let status_str = match st.status {
                                                    rtfs_compiler::ccos::types::IntentStatus::Active => "active",
                                                    rtfs_compiler::ccos::types::IntentStatus::Executing => "executing",
                                                    rtfs_compiler::ccos::types::IntentStatus::Completed => "completed",
                                                    rtfs_compiler::ccos::types::IntentStatus::Failed => "failed",
                                                    rtfs_compiler::ccos::types::IntentStatus::Archived => "archived",
                                                    rtfs_compiler::ccos::types::IntentStatus::Suspended => "suspended",
                                                };

                                                // Special handling for root node
                                                let is_root = root_node_id.as_ref() == Some(intent_id);
                                                let (node_label, execution_order_value) = if is_root {
                                                    // Root node: no execution order, special label
                                                    (format!("🎯 {}", base_label), serde_json::Value::Null)
                                                } else {
                                                    // Child nodes: execution order with number
                                                    (format!("{}. {}", execution_index, base_label), serde_json::json!(execution_index))
                                                };

                                                nodes.push(serde_json::json!({
                                                    "id": st.intent_id.clone(),
                                                    "label": node_label,
                                                    "goal": st.goal.clone(),
                                                    "type": if is_root { "root_intent" } else { "intent" },
                                                    "status": status_str,
                                                    "created_at": st.created_at,
                                                    "execution_order": execution_order_value,
                                                    "is_root": is_root
                                                }));

                                                if !is_root {
                                                    execution_index += 1;
                                                }
                                            }
                                        }

                                        let child_nodes_count = sorted_order.len() - if root_node_id.is_some() { 1 } else { 0 };
                                        if root_node_id.is_some() {
                                            println!("🔢 Execution order established: {} child nodes numbered 1-{}, root node separate", child_nodes_count, child_nodes_count);
                                        } else {
                                            println!("🔢 Execution order established: {} nodes numbered 1-{}", sorted_order.len(), sorted_order.len());
                                        }

                                        // Debug: Check for duplicate node IDs
                                        let node_ids: Vec<String> = nodes.iter().map(|n| n["id"].as_str().unwrap_or("").to_string()).collect();
                                        let unique_ids: std::collections::HashSet<String> = node_ids.iter().cloned().collect();
                                        if node_ids.len() != unique_ids.len() {
                                            println!("❌ SERVER: DUPLICATE NODE IDs DETECTED!");
                                            println!("  Total nodes: {}", node_ids.len());
                                            println!("  Unique IDs: {}", unique_ids.len());
                                            println!("  All IDs: {:?}", node_ids);
                                            // Find duplicates
                                            use std::collections::HashMap;
                                            let mut counts = HashMap::new();
                                            for id in &node_ids {
                                                *counts.entry(id.clone()).or_insert(0) += 1;
                                            }
                                            for (id, count) in counts {
                                                if count > 1 {
                                                    println!("  DUPLICATE: {} appears {} times", id, count);
                                                }
                                            }
                                        } else {
                                            println!("✅ SERVER: All node IDs are unique ({} nodes)", node_ids.len());
                                        }

                                        // Create edges for parent-child relationships
                                        // Only create edges from parents to children to avoid duplicates
                                        println!("🔗 Creating edges for {} intents...", sorted_order.len());
                                        for intent_id in &sorted_order {
                                            if let Some(st) = intent_map.get(intent_id.as_str()) {
                                                // Only create edges when processing parents (not when processing children)
                                                let child_intents = graph_lock.get_child_intents(&st.intent_id);
                                                if !child_intents.is_empty() {
                                                    println!("   {} has {} children:", st.intent_id, child_intents.len());
                                                }
                                                for child in child_intents {
                                                    let _edge_id = format!("{}--{}", st.intent_id, child.intent_id);
                                                    println!("     Edge: {} -> {}", st.intent_id, child.intent_id);
                                                    edges.push(serde_json::json!({
                                                        "source": st.intent_id.clone(),
                                                        "target": child.intent_id,
                                                        "type": "depends_on"
                                                    }));
                                                }
                                            }
                                        }

                                        // Debug: Check for duplicate edges
                                        let edge_ids: Vec<String> = edges.iter().map(|e| {
                                            format!("{}--{}", e["source"].as_str().unwrap_or(""), e["target"].as_str().unwrap_or(""))
                                        }).collect();
                                        let unique_edge_ids: std::collections::HashSet<String> = edge_ids.iter().cloned().collect();
                                        if edge_ids.len() != unique_edge_ids.len() {
                                            println!("❌ SERVER: DUPLICATE EDGES DETECTED!");
                                            println!("  Total edges: {}", edge_ids.len());
                                            println!("  Unique edges: {}", unique_edge_ids.len());
                                            println!("  All edge IDs: {:?}", edge_ids);
                                            // Find duplicates
                                            use std::collections::HashMap;
                                            let mut counts = HashMap::new();
                                            for id in &edge_ids {
                                                *counts.entry(id.clone()).or_insert(0) += 1;
                                            }
                                            for (id, count) in counts {
                                                if count > 1 {
                                                    println!("  DUPLICATE: {} appears {} times", id, count);
                                                }
                                            }
                                        } else {
                                            println!("✅ SERVER: All edge IDs are unique ({} edges)", edge_ids.len());
                                        }

                                        println!("📤 Sending successful response with {} nodes in execution order and {} edges", nodes.len(), edges.len());
                            let _ = req.resp.send(Ok((root_id, nodes, edges)));
                                    } else {
                                        // Handle the case where we couldn't get the graph lock
                                        let _ = req.resp.send(Err("Failed to access intent graph".to_string()));
                                    }
                        }
                        Err(e) => {
                                    println!("❌ Arbiter error during graph generation: {}", e);
                            let _ = req.resp.send(Err(format!("arbiter error: {}", e)));
                                }
                            }
                        } else {
                            // No arbiter available
                            println!("❌ No delegating arbiter available for graph generation");
                            let _ = req.resp.send(Err("no delegating arbiter available".to_string()));
                        }
                    }

                    Some(req) = plan_rx.recv() => {
                        println!("🔄 Processing plan generation request in worker thread");
                        let graph_id = req.graph_id.clone();

                        // Generate plans for leaf intents in the graph
                        println!("🔍 Checking for delegating arbiter for plan generation...");
                        if let Some(arb) = ccos.get_delegating_arbiter() {
                            println!("✅ Delegating arbiter found for plan generation");
                            let mut plans = Vec::new();

                            if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                                let all = graph_lock.storage.get_all_intents_sync();
                                
                                // Filter intents with matching graph_id
                                let intents_in_graph: Vec<_> = all.into_iter()
                                    .filter(|st| st.metadata.get("graph_id").map(|v| v == &graph_id).unwrap_or(false))
                                    .collect();

                                let intent_ids_in_graph: std::collections::HashSet<String> = intents_in_graph.iter().map(|i| i.intent_id.clone()).collect();
                                let mut non_leaves: std::collections::HashSet<String> = std::collections::HashSet::new();

                                for intent in &intents_in_graph {
                                    let children = graph_lock.get_child_intents(&intent.intent_id);
                                    for child in children {
                                        if intent_ids_in_graph.contains(&child.intent_id) {
                                            non_leaves.insert(intent.intent_id.clone());
                                        }
                                    }
                                }

                                let leaf_intents: Vec<_> = intents_in_graph.into_iter()
                                    .filter(|i| {
                                        // Exclude root intents (they don't need plans)
                                        let is_root = i.name.as_ref().map(|n| n == "Root").unwrap_or(false) || 
                                                     i.intent_id == graph_id;
                                        let is_leaf = !non_leaves.contains(&i.intent_id);
                                        // Only include leaf intents that are not root
                                        is_leaf && !is_root
                                    })
                                    .collect();

                                println!("📋 Found {} leaf intents to generate plans for (graph {})", leaf_intents.len(), graph_id);

                                for st in leaf_intents {
                                    println!("🎯 Generating plan for intent: {} - \"{}\"", st.intent_id, st.goal);
                                    match arb.generate_plan_for_intent(&st).await {
                                        Ok(result) => {
                                            println!("✅ Successfully generated plan for intent: {}", st.intent_id);
                                            let body = match &result.plan.body {
                                                rtfs_compiler::ccos::types::PlanBody::Rtfs(txt) => {
                                                    println!("📝 Plan body (first 200 chars): {}", txt.chars().take(200).collect::<String>());
                                                    txt.clone()
                                                },
                                                _ => {
                                                    println!("⚠️ Non-RTFS plan body type");
                                                    "<non-RTFS plan>".to_string()
                                                },
                                            };

                                            // Store the plan in CCOS plan archive
                                            match ccos.get_orchestrator().store_plan(&result.plan) {
                                                Ok(archive_hash) => {
                                                    println!("💾 Stored plan {} in archive with hash: {}", result.plan.plan_id, archive_hash);
                                                },
                                                Err(e) => {
                                                    println!("⚠️ Failed to store plan {} in archive: {}", result.plan.plan_id, e);
                                                }
                                            }

                                            plans.push(serde_json::json!({
                                                "intent_id": st.intent_id,
                                                "plan_id": result.plan.plan_id,
                                                "body": body,
                                                "status": "generated"
                                            }));
                                        }
                                        Err(e) => {
                                            println!("❌ Failed to generate plan for {}: {}", st.intent_id, e);
                                        }
                                    }
                                }
                            }

                            println!("📤 Sending plan generation response with {} plans", plans.len());
                            let _ = req.resp.send(Ok(plans));
                        } else {
                            println!("❌ No delegating arbiter available for plan generation");
                let _ = req.resp.send(Err("no delegating arbiter available".to_string()));
                        }
                    }

                    Some(req) = execute_rx.recv() => {
                        let graph_id = req.graph_id.clone();

                        // Execute the intent graph using the orchestrator
                        if let Some(_arb) = ccos.get_delegating_arbiter() {
                            let ctx = runtime_service::default_controlled_context();

                            match ccos.get_orchestrator().execute_intent_graph(&graph_id, &ctx).await {
                                Ok(result) => {
                                    let result_str = format!("Execution result: {}", result.value);
                                    let _ = req.resp.send(Ok(result_str));
                        }
                        Err(e) => {
                                    let _ = req.resp.send(Err(format!("execution error: {}", e)));
                                }
                            }
                        } else {
                            let _ = req.resp.send(Err("no delegating arbiter available".to_string()));
                        }
                    }

                    Some(req) = load_graph_rx.recv() => {
                        println!("🔄 Processing load graph request in worker thread");
                        println!("📊 Loading graph with {} nodes and {} edges",
                                 req.nodes.len(), req.edges.len());

                        // Determine graph_id (prefer provided root_id)
                        let graph_id = req.root_id.clone().unwrap_or_else(|| format!("loaded_graph_{}", chrono::Utc::now().timestamp()));
                        println!("📝 Using graph ID: {}", graph_id);

                        // Rehydrate intents into CCOS
                        if let Ok(mut graph_lock) = ccos.get_intent_graph().lock() {
                            // Insert intents
                            for node in &req.nodes {
                                if let (Some(id), Some(goal)) = (node.get("id").and_then(|v| v.as_str()), node.get("goal").and_then(|v| v.as_str())) {
                                    // Skip if already present
                                    if graph_lock.get_intent(&id.to_string()).is_some() {
                                        continue;
                                    }
                                    let mut st = rtfs_compiler::ccos::types::StorableIntent::new(goal.to_string());
                                    st.intent_id = id.to_string();
                                    st.metadata.insert("graph_id".to_string(), graph_id.clone());
                                    // Mark root intent specially (no plan)
                                    if let Some(is_root) = node.get("is_root").and_then(|v| v.as_bool()) {
                                        if is_root {
                                            st.name = Some("Root".to_string());
                                        }
                                    }
                                    if let Err(e) = graph_lock.store_intent(st) {
                                        println!("⚠️ Failed to store intent {}: {}", id, e);
                                    }
                                }
                            }

                            // Insert edges
                            for edge in &req.edges {
                                if let (Some(from), Some(to)) = (edge.get("source").and_then(|v| v.as_str()), edge.get("target").and_then(|v| v.as_str())) {
                                    let edge_type = match edge.get("type").and_then(|v| v.as_str()) {
                                        Some("depends_on") => rtfs_compiler::ccos::types::EdgeType::DependsOn,
                                        Some("is_subgoal_of") => rtfs_compiler::ccos::types::EdgeType::IsSubgoalOf,
                                        Some("conflicts_with") => rtfs_compiler::ccos::types::EdgeType::ConflictsWith,
                                        Some("enables") => rtfs_compiler::ccos::types::EdgeType::Enables,
                                        Some("related_to") => rtfs_compiler::ccos::types::EdgeType::RelatedTo,
                                        _ => rtfs_compiler::ccos::types::EdgeType::DependsOn,
                                    };
                                    if let Err(e) = graph_lock.create_edge(from.to_string(), to.to_string(), edge_type) {
                                        println!("⚠️ Failed to create edge {} -> {}: {}", from, to, e);
                                    }
                                }
                            }
                        }

                        let _ = req.resp.send(Ok(graph_id));
                    }

                    Some(req) = get_plans_req_rx.recv() => {
                        println!("🔄 Processing get plans request for graph: {}", req.graph_id);
                        
                        // Get all intents for this graph
                        if let Ok(graph_lock) = ccos.get_intent_graph().lock() {
                            let all = graph_lock.storage.get_all_intents_sync();
                            
                            // Filter intents with matching graph_id
                            let intents_in_graph: Vec<_> = all.into_iter()
                                .filter(|st| st.metadata.get("graph_id").map(|v| v == &req.graph_id).unwrap_or(false))
                                .collect();

                            println!("🔍 Found {} intents in graph {}", intents_in_graph.len(), req.graph_id);

                            // Get plans for each intent from the plan archive
                            let mut plans = Vec::new();
                            for intent in &intents_in_graph {
                                // Skip root intents (they don't have plans)
                                if intent.name.as_ref().map(|n| n == "Root").unwrap_or(false) || 
                                   intent.intent_id == req.graph_id {
                                    continue;
                                }

                                // Get plans for this intent from the archive
                                let archivable_plans = ccos.get_orchestrator().get_plan_for_intent(&intent.intent_id).ok().flatten();
                                if let Some(plan) = archivable_plans {
                                    let body = match &plan.body {
                                        rtfs_compiler::ccos::types::PlanBody::Rtfs(txt) => txt.clone(),
                                        _ => "<non-RTFS plan>".to_string(),
                                    };

                                    plans.push(serde_json::json!({
                                        "intent_id": intent.intent_id,
                                        "plan_id": plan.plan_id,
                                        "body": body,
                                        "status": "retrieved"
                                    }));
                                    
                                    println!("📋 Retrieved plan for intent: {}", intent.intent_id);
                                }
                            }

                            println!("📋 Retrieved {} plans for graph {}", plans.len(), req.graph_id);
                            let _ = req.resp.send(Ok(plans));
                        } else {
                            let _ = req.resp.send(Err("Failed to lock intent graph".to_string()));
                        }
                    }

                    else => break,
                }
            }
        });
    });

    let state = Arc::new(AppState {
        tx,
        graph_req_tx,
        plan_req_tx,
        execute_req_tx,
        load_graph_req_tx,
        get_plans_req_tx,
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
        .route("/load-graph", post(load_graph_handler))
        .route("/get-plans", post(get_plans_handler))
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
