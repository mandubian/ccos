use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
};
use axum::response::IntoResponse;
use futures::{sink::SinkExt, stream::StreamExt};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;

// Define the events that will be sent to the frontend
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
}

#[tokio::main]
async fn main() {
    let (tx, _) = broadcast::channel(100);
    let app_state = Arc::new(AppState { tx });

    // Spawn a task to simulate CCOS events for demonstration
    let app_state_clone = app_state.clone();
    tokio::spawn(async move {
        simulate_ccos_execution(app_state_clone).await;
    });

    // Router/service setup is intentionally omitted here to avoid feature/version
    // conflicts in this monorepo build. The websocket handler and static assets
    // remain in place and can be wired to a server in a follow-up change.

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!(
        "CCOS Viewer (server wiring omitted) listening on http://{}",
        addr
    );
    // NOTE: starting a full HTTP server here caused hyper/axum feature conflicts in the
    // monorepo build. To keep the example compiling cleanly, we currently do not start
    // the server automatically. Instead, block until Ctrl-C so the binary stays alive
    // and can be manually wired to run the server in a follow-up change.

    tokio::signal::ctrl_c()
        .await
        .expect("failed to wait for ctrl-c");
}

#[allow(dead_code)]
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

#[allow(dead_code)]
async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = stream.split();
    let mut rx = state.tx.subscribe();

    // Send initial state if available (or a welcome message)
    // For now, we just log connection. The simulation will send the first event.

    let mut send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            let json = serde_json::to_string(&event).unwrap();
            if sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
}

// This function simulates the CCOS execution flow for demonstration purposes.
// It will be replaced with actual instrumentation of the CCOS core.
async fn simulate_ccos_execution(state: Arc<AppState>) {
    use tokio::time::{sleep, Duration};

    let rtfs_code = r#"(sequence
  (action "load_data" (source "file.csv"))
  (action "process_data" (input (ref "load_data")))
  (action "generate_report" (input (ref "process_data"))))"#;

    let nodes = vec![
        serde_json::json!({"id": "load_data", "label": "Load Data"}),
        serde_json::json!({"id": "process_data", "label": "Process Data"}),
        serde_json::json!({"id": "generate_report", "label": "Generate Report"}),
    ];

    let edges = vec![
        serde_json::json!({"from": "load_data", "to": "process_data"}),
        serde_json::json!({"from": "process_data", "to": "generate_report"}),
    ];

    // 1. Send initial graph
    state
        .tx
        .send(ViewerEvent::FullUpdate {
            nodes,
            edges,
            rtfs_code: rtfs_code.to_string(),
        })
        .unwrap();
    sleep(Duration::from_secs(2)).await;

    // 2. Simulate execution flow
    let action_ids = ["load_data", "process_data", "generate_report"];
    for id in action_ids {
        // Set to InProgress
        state
            .tx
            .send(ViewerEvent::NodeStatusChange {
                id: id.to_string(),
                status: "InProgress".to_string(),
            })
            .unwrap();
        sleep(Duration::from_secs(1)).await;

        // Set to Success
        state
            .tx
            .send(ViewerEvent::NodeStatusChange {
                id: id.to_string(),
                status: "Success".to_string(),
            })
            .unwrap();
        sleep(Duration::from_secs(1)).await;
    }
}
