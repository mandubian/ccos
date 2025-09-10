//! CCOS Runtime Service
//!
//! A thin, embeddable async wrapper around CCOS that exposes a clean
//! command/event interface suitable for CLI, TUI, and Web frontends.
//!
//! Goals
//! - Decouple UI render/input loops from CCOS orchestration
//! - Provide a small surface area: start, cancel, subscribe, shutdown
//! - Preserve CCOS separation of powers: UI sends commands, service drives CCOS
//! - Forward audit/status via IntentEventSink and CausalChain event adapters

use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tokio::sync::{mpsc, broadcast};

use super::types::IntentId;
use super::{CCOS};
use crate::runtime::security::{RuntimeContext, SecurityLevel};
use crate::ccos::event_sink::IntentEventSink;
use crate::runtime::error::RuntimeError;

/// Commands a frontend can send to the runtime service
#[derive(Debug, Clone)]
pub enum RuntimeCommand {
    /// Start processing a new natural language goal
    Start { goal: String, context: RuntimeContext },
    /// Attempt to cancel an in-flight intent/plan by root intent id (best-effort)
    Cancel { intent_id: IntentId },
    /// Graceful shutdown of the service
    Shutdown,
}

/// Events emitted by the runtime service for UI consumption
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Started { intent_id: IntentId, goal: String },
    Status { intent_id: IntentId, status: String },
    Step { intent_id: IntentId, desc: String },
    Result { intent_id: IntentId, result: String }, // RTFS-formatted result string
    Error { message: String },
    Heartbeat,
    Stopped,
    // New events for graph generation and plan execution
    GraphGenerated { root_id: IntentId, nodes: Vec<serde_json::Value>, edges: Vec<serde_json::Value> },
    PlanGenerated { intent_id: IntentId, plan_id: String, rtfs_code: String },
    StepLog { step: String, status: String, message: String, details: Option<serde_json::Value> },
    ReadyForNext { next_step: String },
}

/// Handle returned to callers for interacting with the service
pub struct RuntimeHandle {
    pub cmd_tx: mpsc::Sender<RuntimeCommand>,
    pub evt_tx: broadcast::Sender<RuntimeEvent>,
}

impl RuntimeHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> { self.evt_tx.subscribe() }
    pub fn commands(&self) -> mpsc::Sender<RuntimeCommand> { self.cmd_tx.clone() }
}

/// Start the runtime service. Spawns an internal task and returns a handle.
pub async fn start_service(ccos: Arc<CCOS>) -> RuntimeHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<RuntimeCommand>(64);
    let (evt_tx, _evt_rx) = broadcast::channel::<RuntimeEvent>(256);

    // Install composite sink in current thread
    {
        if let Ok(mut g) = ccos.get_intent_graph().lock() {
            let original = g.intent_event_sink.clone();
            let bcast_sink = Arc::new(BroadcastIntentEventSink::new(evt_tx.clone()));
            g.intent_event_sink = Arc::new(CompositeIntentEventSink::new(vec![original, bcast_sink]));
        }
    }

    // Spawn the service loop on the local task set; caller must use a current_thread runtime
    let evt_tx_for_loop = evt_tx.clone();
    println!("Starting runtime service task...");
    tokio::task::spawn_local(async move {
        println!("Runtime service task started");
        let mut cmd_rx = cmd_rx;
        // Track the currently running request so we can cancel it
        let mut current_task: Option<tokio::task::JoinHandle<()>> = None;
        let mut current_intent_id: Option<IntentId> = None;

        while let Some(cmd) = cmd_rx.recv().await {
            println!("Runtime service received command: {:?}", cmd);
            match cmd {
                RuntimeCommand::Start { goal, context } => {
                    // If a task is already running, abort it to start a fresh one
                    if let Some(handle) = current_task.take() { handle.abort(); }

                    let tx = evt_tx_for_loop.clone();
                    // Generate a temporary intent id now so UI can bind Cancel
                    let tmp_intent_id = format!("pending-{}", uuid::Uuid::new_v4());
                    current_intent_id = Some(tmp_intent_id.clone());

                    // Run the request locally (no Send bound)
                    let ccos_req = Arc::clone(&ccos);
                    let handle = tokio::task::spawn_local(async move {
                        println!("Starting to process request for goal: {}", goal.clone());
                        let _ = tx.send(RuntimeEvent::Started { intent_id: tmp_intent_id.clone(), goal: goal.clone() });
                        // Avoid indefinite hangs: timebox orchestration
                        match timeout(Duration::from_secs(25), ccos_req.process_request(&goal, &context)).await {
                            Ok(Ok(result)) => {
                                println!("Request processed successfully: {:?}", result);
                                let intent_id = ccos_req.get_intent_graph().lock().ok().and_then(|g| {
                                    let intents = g.storage.get_all_intents_sync();
                                    intents.into_iter()
                                        .filter(|i| i.goal == goal)
                                        .max_by_key(|i| i.updated_at)
                                        .map(|i| i.intent_id)
                                }).unwrap_or(tmp_intent_id.clone());
                                // Convert ExecutionResult value to RTFS-formatted string
                                let rtfs_result = if result.success {
                                    format!("{}", result.value)
                                } else {
                                    format!("Error: {}", result.value)
                                };
                                let _ = tx.send(RuntimeEvent::Result { intent_id, result: rtfs_result });
                            }
                            Ok(Err(e)) => {
                                println!("Request processing failed: {:?}", e);
                                let _ = tx.send(RuntimeEvent::Error { message: format!("process_request error: {e}") });
                            }
                            Err(_) => {
                                println!("Request processing timed out");
                                let _ = tx.send(RuntimeEvent::Error { message: "process_request timed out after 25s".to_string() });
                            }
                        }
                    });

                    current_task = Some(handle);
                }
                RuntimeCommand::Cancel { intent_id: _ } => {
                    let msg = if let Some(handle) = current_task.take() {
                        handle.abort();
                        let id = current_intent_id.take().unwrap_or_else(|| "unknown-intent".to_string());
                        format!("Canceled intent {}", id)
                    } else {
                        "No running intent to cancel".to_string()
                    };
                    let _ = evt_tx_for_loop.send(RuntimeEvent::Error { message: msg });
                }
                RuntimeCommand::Shutdown => {
                    if let Some(handle) = current_task.take() { handle.abort(); }
                    let _ = evt_tx_for_loop.send(RuntimeEvent::Stopped);
                    break;
                }
            }
        }
    });

    RuntimeHandle { cmd_tx, evt_tx }
}

/// A minimal helper to make a permissive RuntimeContext quickly
pub fn default_controlled_context() -> RuntimeContext {
    use std::collections::HashSet;
    RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: [
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(), // offline
            // Avoid online/LLM capabilities by default in demos
        ].into_iter().collect::<HashSet<_>>(),
        ..RuntimeContext::controlled(Vec::new())
    }
}

// --- Event sink adapters ---

#[derive(Clone)]
struct BroadcastIntentEventSink {
    tx: broadcast::Sender<RuntimeEvent>,
}

impl std::fmt::Debug for BroadcastIntentEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastIntentEventSink").finish()
    }
}

impl BroadcastIntentEventSink {
    fn new(tx: broadcast::Sender<RuntimeEvent>) -> Self { Self { tx } }
}

impl IntentEventSink for BroadcastIntentEventSink {
    fn log_intent_status_change(
        &self,
        _plan_id: &str,
        intent_id: &IntentId,
        _old_status: &str,
        new_status: &str,
        _reason: &str,
        _triggering_action_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let _ = self.tx.send(RuntimeEvent::Status { intent_id: intent_id.clone(), status: new_status.to_string() });
        Ok(())
    }
}

#[derive(Clone)]
struct CompositeIntentEventSink {
    sinks: Vec<Arc<dyn IntentEventSink>>, 
}

impl std::fmt::Debug for CompositeIntentEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeIntentEventSink").finish()
    }
}

impl CompositeIntentEventSink {
    fn new(sinks: Vec<Arc<dyn IntentEventSink>>) -> Self { Self { sinks } }
}

impl IntentEventSink for CompositeIntentEventSink {
    fn log_intent_status_change(
        &self,
        plan_id: &str,
        intent_id: &IntentId,
        old_status: &str,
        new_status: &str,
        reason: &str,
        triggering_action_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        for s in &self.sinks {
            // Best-effort: forward to all, ignore individual sink errors to keep UI responsive
            let _ = s.log_intent_status_change(plan_id, intent_id, old_status, new_status, reason, triggering_action_id);
        }
        Ok(())
    }
}
