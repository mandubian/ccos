//! Chat mode (Phase 0): quarantine + classification + egress gating.
//!
//! This module is the minimal enforceable implementation of specs:
//! - 037-chat-mode-security-contract.md
//! - 038-chat-mode-policy-pack.md
//! - 039-quarantine-store-contract.md
//! - 044-hello-world-connector-flow.md
//! - 045-chat-transform-capabilities.md
//! - 046-chat-approval-flow.md
//! - 047-chat-audit-events.md

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use sha2::Digest;
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use base64::Engine as _;

use crate::approval::types::{ApprovalCategory, RiskAssessment, RiskLevel};
use crate::approval::UnifiedApprovalQueue;
use crate::approval::{storage_file::FileApprovalStorage, ApprovalAuthority};
use crate::causal_chain::CausalChain;
use crate::types::{Action, ActionType};
use crate::utils::value_conversion::{json_to_rtfs_value, map_key_to_string, rtfs_value_to_json};

use crate::chat::connector::{ChatConnector, ConnectionHandle, OutboundRequest};

use crate::capability_marketplace::types::{
    CapabilityManifest, EffectType, NativeCapability, ProviderType,
};

pub mod agent_llm;
pub mod agent_log;
pub mod agent_monitor;
pub mod checkpoint;
pub mod connector;
pub mod gateway;
pub mod quarantine;
pub mod predicate;
pub mod realtime_sink;
pub mod resource;
pub mod run;
pub mod scheduler;
pub mod session;
pub mod spawner;
#[cfg(test)]
mod checkpoint_tests;

pub use connector::{ActivationMetadata, AttachmentRef, MessageDirection, MessageEnvelope};
pub use predicate::Predicate;
pub use quarantine::{FileQuarantineStore, InMemoryQuarantineStore, QuarantineKey, QuarantineStore};
pub use resource::{new_shared_resource_store, ResourceRecord, ResourceStore, SharedResourceStore};
pub use checkpoint::{Checkpoint, CheckpointStore, InMemoryCheckpointStore};
pub use realtime_sink::{RealTimeTrackingSink, SessionEvent, SessionStateSnapshot, ActionView};
pub use run::{BudgetContext, Run, RunState, RunStore, SharedRunStore, new_shared_run_store};
pub use scheduler::Scheduler;
pub use session::{ChatMessage, SessionRegistry};
pub use spawner::{AgentSpawner, SpawnConfig, SpawnerFactory};
pub use agent_monitor::{AgentMonitor, AgentHealth};

/// Reserved key for CCOS-internal chat metadata inside RTFS values.
///
/// This is stripped before any egress to external agents/channels.
pub const CCOS_META_KEY: &str = "__ccos_meta";

const META_CLASS_KEY: &str = "class";
const META_FIELD_LABELS_KEY: &str = "field_labels";

/// Chat-mode data classification label (spec 037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatDataLabel {
    Public,
    PiiRedacted,
    PiiChatMessage,
    PiiChatMetadata,
    PiiAttachment,
    SecretToken,
    InternalSystem,
}

impl ChatDataLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            ChatDataLabel::Public => "public",
            ChatDataLabel::PiiRedacted => "pii.redacted",
            ChatDataLabel::PiiChatMessage => "pii.chat.message",
            ChatDataLabel::PiiChatMetadata => "pii.chat.metadata",
            ChatDataLabel::PiiAttachment => "pii.attachment",
            ChatDataLabel::SecretToken => "secret.token",
            ChatDataLabel::InternalSystem => "internal.system",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "public" => Some(ChatDataLabel::Public),
            "pii.redacted" => Some(ChatDataLabel::PiiRedacted),
            "pii.chat.message" => Some(ChatDataLabel::PiiChatMessage),
            "pii.chat.metadata" => Some(ChatDataLabel::PiiChatMetadata),
            "pii.attachment" => Some(ChatDataLabel::PiiAttachment),
            "secret.token" => Some(ChatDataLabel::SecretToken),
            "internal.system" => Some(ChatDataLabel::InternalSystem),
            _ => None,
        }
    }

    /// Join operation from spec 037.
    pub fn join(a: ChatDataLabel, b: ChatDataLabel) -> ChatDataLabel {
        if a == ChatDataLabel::SecretToken || b == ChatDataLabel::SecretToken {
            return ChatDataLabel::SecretToken;
        }

        // Treat internal.system as non-exportable (effectively restrictive for egress),
        // but do not let it "taint" user data upward inside computation.
        if a == ChatDataLabel::InternalSystem {
            return b;
        }
        if b == ChatDataLabel::InternalSystem {
            return a;
        }

        // Public < pii.redacted < pii.chat.* / pii.attachment
        use ChatDataLabel::*;
        match (a, b) {
            (Public, x) | (x, Public) => x,
            (PiiRedacted, x) | (x, PiiRedacted) => x,

            // Within pii raw classes, pick the "more raw" one deterministically.
            (PiiChatMessage, PiiAttachment) | (PiiAttachment, PiiChatMessage) => PiiAttachment,
            (PiiChatMetadata, PiiAttachment) | (PiiAttachment, PiiChatMetadata) => PiiAttachment,
            (PiiChatMetadata, PiiChatMessage) | (PiiChatMessage, PiiChatMetadata) => PiiChatMessage,
            (x, y) if x == y => x,

            // Fallback: treat as most restrictive PII variant.
            _ => PiiAttachment,
        }
    }
}

/// Extract a chat classification label from a Value's `__ccos_meta`.
/// If absent or invalid, defaults to `pii.chat.message` (spec 037: unknowns default to PII).
pub fn extract_label(value: &Value) -> ChatDataLabel {
    let default_label = ChatDataLabel::PiiChatMessage;
    let Some(meta) = get_meta_map(value) else {
        return default_label;
    };

    let class_label = meta
        .get(META_CLASS_KEY)
        .and_then(|class_val| class_val.as_string())
        .and_then(ChatDataLabel::parse)
        .unwrap_or(default_label);

    let field_labels = extract_field_labels(&meta);
    if field_labels.is_empty() {
        return class_label;
    }

    let Value::Map(map) = value else {
        return class_label;
    };

    let mut joined = class_label;
    for (k, _v) in map.iter() {
        if is_meta_key(k) {
            continue;
        }
        let key = map_key_to_string(k);
        let field_label = field_labels.get(&key).copied().unwrap_or(class_label);
        joined = ChatDataLabel::join(joined, field_label);
    }

    joined
}

/// Strip CCOS-internal metadata from a value recursively.
pub fn strip_ccos_meta(value: &Value) -> Value {
    match value {
        Value::Map(map) => {
            let mut out: HashMap<MapKey, Value> = HashMap::new();
            for (k, v) in map.iter() {
                let is_meta = matches!(k, MapKey::String(s) if s == CCOS_META_KEY)
                    || matches!(k, MapKey::Keyword(kw) if kw.0 == CCOS_META_KEY);
                if is_meta {
                    continue;
                }
                out.insert(k.clone(), strip_ccos_meta(v));
            }
            Value::Map(out)
        }
        Value::Vector(v) => Value::Vector(v.iter().map(strip_ccos_meta).collect()),
        Value::List(v) => Value::List(v.iter().map(strip_ccos_meta).collect()),
        other => other.clone(),
    }
}

fn get_meta_map(value: &Value) -> Option<HashMap<String, Value>> {
    let Value::Map(map) = value else { return None };
    let meta = map.get(&MapKey::String(CCOS_META_KEY.to_string()))
        .or_else(|| map.get(&MapKey::Keyword(Keyword(CCOS_META_KEY.to_string()))))?;
    let Value::Map(inner) = meta else { return None };

    let mut out = HashMap::new();
    for (k, v) in inner.iter() {
        let key = match k {
            MapKey::String(s) => s.clone(),
            MapKey::Keyword(kw) => kw.0.clone(),
            MapKey::Integer(i) => i.to_string(),
        };
        out.insert(key, v.clone());
    }
    Some(out)
}

fn with_meta(
    mut value: Value,
    class: ChatDataLabel,
    field_labels: Option<HashMap<String, ChatDataLabel>>,
) -> Value {
    let mut meta_map: HashMap<MapKey, Value> = HashMap::new();
    meta_map.insert(
        MapKey::String(META_CLASS_KEY.to_string()),
        Value::String(class.as_str().to_string()),
    );
    meta_map.insert(
        MapKey::Keyword(Keyword(META_CLASS_KEY.to_string())),
        Value::String(class.as_str().to_string()),
    );

    if let Some(field_labels) = field_labels {
        let mut labels_map: HashMap<MapKey, Value> = HashMap::new();
        for (k, lbl) in field_labels {
            labels_map.insert(MapKey::String(k.clone()), Value::String(lbl.as_str().to_string()));
            labels_map.insert(MapKey::Keyword(Keyword(k)), Value::String(lbl.as_str().to_string()));
        }
        meta_map.insert(
            MapKey::String(META_FIELD_LABELS_KEY.to_string()),
            Value::Map(labels_map.clone()),
        );
        meta_map.insert(
            MapKey::Keyword(Keyword(META_FIELD_LABELS_KEY.to_string())),
            Value::Map(labels_map),
        );
    }

    match &mut value {
        Value::Map(map) => {
            map.insert(MapKey::String(CCOS_META_KEY.to_string()), Value::Map(meta_map.clone()));
            map.insert(MapKey::Keyword(Keyword(CCOS_META_KEY.to_string())), Value::Map(meta_map));
            value
        }
        other => {
            // For non-map outputs, wrap into a map so metadata is representable.
            let mut map: HashMap<MapKey, Value> = HashMap::new();
            map.insert(MapKey::String("value".to_string()), other.clone());
            map.insert(MapKey::Keyword(Keyword("value".to_string())), other.clone());
            map.insert(MapKey::String(CCOS_META_KEY.to_string()), Value::Map(meta_map.clone()));
            map.insert(MapKey::Keyword(Keyword(CCOS_META_KEY.to_string())), Value::Map(meta_map));
            Value::Map(map)
        }
    }
}

/// Attach classification metadata to a value.
pub fn attach_label(
    value: Value,
    class: ChatDataLabel,
    field_labels: Option<HashMap<String, ChatDataLabel>>,
) -> Value {
    with_meta(value, class, field_labels)
}

fn is_meta_key(key: &MapKey) -> bool {
    map_key_to_string(key) == CCOS_META_KEY
}

fn extract_field_labels(meta: &HashMap<String, Value>) -> HashMap<String, ChatDataLabel> {
    let mut out = HashMap::new();
    let Some(Value::Map(labels)) = meta.get(META_FIELD_LABELS_KEY) else {
        return out;
    };
    for (k, v) in labels.iter() {
        let label_str = v.as_string().unwrap_or_default();
        if let Some(label) = ChatDataLabel::parse(label_str) {
            out.insert(map_key_to_string(k), label);
        }
    }
    out
}


/// Records a chat audit event into the causal chain.
///
/// This is the Phase 0 minimal enforcement backing `047-chat-audit-events.md`.
///
/// `action_type` controls how the event appears in the monitor: use
/// `ActionType::CapabilityCall` for direct capability executions (visible by
/// default) and `ActionType::InternalStep` for internal bookkeeping (hidden
/// unless the monitor's "show internal steps" toggle is on).
pub fn record_chat_audit_event(
    chain: &Arc<Mutex<CausalChain>>,
    plan_id: &str,
    intent_id: &str,
    session_id: &str,
    run_id: &str,
    step_id: &str,
    event_type: &str,
    mut metadata: HashMap<String, Value>,
    action_type: ActionType,
) -> RuntimeResult<()> {
    metadata.insert("event_type".to_string(), Value::String(event_type.to_string()));
    metadata.insert("session_id".to_string(), Value::String(session_id.to_string()));
    metadata.insert("run_id".to_string(), Value::String(run_id.to_string()));
    metadata.insert("step_id".to_string(), Value::String(step_id.to_string()));

    // For CapabilityCall actions use the actual capability_id as the function name so the
    // monitor event pane shows e.g. "ccos.execute.python" instead of "chat.audit.<event>".
    let function_name = metadata
        .get("capability_id")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("chat.audit.{}", event_type));

    let action = Action {
        action_id: uuid::Uuid::new_v4().to_string(),
        parent_action_id: None,
        session_id: Some(session_id.to_string()),
        plan_id: plan_id.to_string(),
        intent_id: intent_id.to_string(),
        action_type,
        function_name: Some(function_name),
        arguments: None,
        result: None,
        cost: None,
        duration_ms: None,
        timestamp: Utc::now().timestamp_millis() as u64,
        metadata,
    };

    let mut guard = chain.lock().map_err(|_| {
        RuntimeError::Generic("Failed to lock CausalChain for chat audit".to_string())
    })?;
    let _ = guard.append(&action)?;
    Ok(())
}

/// Register Phase 0 chat capabilities into the native provider.
///
/// These capabilities are intentionally narrow and safe:
/// - `ccos.chat.transform.*` read quarantine (by pointer) and return `pii.redacted` outputs.
/// - `ccos.chat.transform.verify_redaction` is required to produce `public` outputs.
/// - `ccos.chat.egress.prepare_outbound` enforces deny-by-default egress rules and strips `__ccos_meta`.
pub async fn register_chat_capabilities(
    marketplace: Arc<crate::capability_marketplace::CapabilityMarketplace>,
    quarantine: Arc<dyn QuarantineStore>,
    causal_chain: Arc<Mutex<CausalChain>>,
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    resource_store: SharedResourceStore,
    connector: Option<Arc<dyn ChatConnector>>,
    connector_handle: Option<ConnectionHandle>,
    gateway_url: Option<String>,
    internal_secret: Option<String>,
    sandbox_config: crate::config::types::SandboxConfig,
    coding_agents_config: crate::config::types::CodingAgentsConfig,
) -> RuntimeResult<()> {
    async fn register_native_chat_capability(
        marketplace: &crate::capability_marketplace::CapabilityMarketplace,
        id: &str,
        name: &str,
        description: &str,
        handler: Arc<
            dyn Fn(&Value) -> futures::future::BoxFuture<'static, RuntimeResult<Value>>
                + Send
                + Sync,
        >,
        security_level: &str,
        effects: Vec<String>,
        effect_type: EffectType,
    ) -> RuntimeResult<()> {
        let mut metadata = HashMap::new();
        metadata.insert("name".to_string(), name.to_string());
        metadata.insert("description".to_string(), description.to_string());
        metadata.insert("security_level".to_string(), security_level.to_string());

        let manifest = CapabilityManifest {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            provider: ProviderType::Native(NativeCapability {
                handler,
                security_level: security_level.to_string(),
                metadata: metadata.clone(),
            }),
            version: "0.1.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects,
            metadata,
            agent_metadata: None,
            domains: vec!["chat".to_string()],
            categories: vec!["transform".to_string()],
            effect_type,
            approval_status: crate::capability_marketplace::types::ApprovalStatus::Approved,
        };

        marketplace.register_capability_manifest(manifest).await
    }

    // ---------------------------------------------------------------------
    // Summarize message (minimal safe summarizer; no raw quotes).
    // ---------------------------------------------------------------------
    {
        let quarantine = Arc::clone(&quarantine);
        let chain = Arc::clone(&causal_chain);
        register_native_chat_capability(
            &*marketplace,
            "ccos.chat.transform.summarize_message",
            "Summarize Message (chat mode)",
            "Read quarantined message by pointer and return pii.redacted summary.",
            Arc::new(move |inputs: &Value| {
                let quarantine = Arc::clone(&quarantine);
                let chain = Arc::clone(&chain);
                let inputs = inputs.clone();
                Box::pin(async move {
                    let (pointer_id, justification, session_id, run_id, step_id) =
                        parse_common_transform_inputs(&inputs)?;

                    // Audit quarantine access (047.quarantine.access)
                    let mut meta = HashMap::new();
                    meta.insert("pointer_id".to_string(), Value::String(pointer_id.clone()));
                    meta.insert(
                        "capability_id".to_string(),
                        Value::String("ccos.chat.transform.summarize_message".to_string()),
                    );
                    meta.insert("capability_version".to_string(), Value::String("0.1".to_string()));
                    meta.insert("justification".to_string(), Value::String(justification));
                    meta.insert("policy_pack_version".to_string(), Value::String("chat-mode-v0".to_string()));
                    meta.insert("rule_id".to_string(), Value::String("chat.quarantine.access".to_string()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "quarantine.access",
                        meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    let bytes = quarantine.get_bytes(&pointer_id)?;
                    let raw = String::from_utf8_lossy(&bytes);

                    // Very conservative "summary": do not quote input; just indicate receipt and length.
                    let summary = format!("Received a message ({} bytes).", bytes.len());

                    // Derived tags (internal only; avoid content).
                    let topics: Vec<Value> = Vec::new();
                    let tasks: Vec<Value> = Vec::new();

                    let mut out = HashMap::new();
                    out.insert(
                        MapKey::String("summary".to_string()),
                        Value::String(summary),
                    );
                    out.insert(
                        MapKey::Keyword(Keyword("summary".to_string())),
                        out[&MapKey::String("summary".to_string())].clone(),
                    );
                    out.insert(MapKey::String("topics".to_string()), Value::Vector(topics));
                    out.insert(
                        MapKey::Keyword(Keyword("topics".to_string())),
                        out[&MapKey::String("topics".to_string())].clone(),
                    );
                    out.insert(MapKey::String("tasks".to_string()), Value::Vector(tasks));
                    out.insert(
                        MapKey::Keyword(Keyword("tasks".to_string())),
                        out[&MapKey::String("tasks".to_string())].clone(),
                    );

                    let _ = raw; // keep raw out of outputs/logs
                    // Audit transform output (047.transform.output)
                    let mut out_meta = HashMap::new();
                    out_meta.insert(
                        "capability_id".to_string(),
                        Value::String("ccos.chat.transform.summarize_message".to_string()),
                    );
                    out_meta.insert("capability_version".to_string(), Value::String("0.1".to_string()));
                    out_meta.insert(
                        "output_schema".to_string(),
                        Value::String("{summary:string, topics?:string[], tasks?:string[]}".to_string()),
                    );
                    out_meta.insert(
                        "output_classification".to_string(),
                        Value::String("pii.redacted".to_string()),
                    );
                    out_meta.insert(
                        "policy_pack_version".to_string(),
                        Value::String("chat-mode-v0".to_string()),
                    );
                    out_meta.insert("rule_id".to_string(), Value::String("chat.transform.output".to_string()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "transform.output",
                        out_meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    let val = Value::Map(out);
                    Ok(attach_label(val, ChatDataLabel::PiiRedacted, None))
                })
            }),
            "low",
            vec!["read".to_string(), "compute".to_string()],
            EffectType::Pure,
        )
        .await?;
    }

    // ---------------------------------------------------------------------
    // Extract entities/tasks (minimal safe extractor).
    // ---------------------------------------------------------------------
    {
        let quarantine = Arc::clone(&quarantine);
        let chain = Arc::clone(&causal_chain);
        register_native_chat_capability(
            &*marketplace,
            "ccos.chat.transform.extract_entities",
            "Extract Entities/Tasks (chat mode)",
            "Read quarantined message by pointer and return pii.redacted entities/tasks.",
            Arc::new(move |inputs: &Value| {
                let quarantine = Arc::clone(&quarantine);
                let chain = Arc::clone(&chain);
                let inputs = inputs.clone();
                Box::pin(async move {
                    let (pointer_id, justification, session_id, run_id, step_id) =
                        parse_common_transform_inputs(&inputs)?;

                    let mut meta = HashMap::new();
                    meta.insert("pointer_id".to_string(), Value::String(pointer_id.clone()));
                    meta.insert(
                        "capability_id".to_string(),
                        Value::String("ccos.chat.transform.extract_entities".to_string()),
                    );
                    meta.insert("capability_version".to_string(), Value::String("0.1".to_string()));
                    meta.insert("justification".to_string(), Value::String(justification));
                    meta.insert("policy_pack_version".to_string(), Value::String("chat-mode-v0".to_string()));
                    meta.insert("rule_id".to_string(), Value::String("chat.quarantine.access".to_string()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "quarantine.access",
                        meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    let bytes = quarantine.get_bytes(&pointer_id)?;
                    let _ = bytes; // keep raw out of outputs/logs

                    let mut out = HashMap::new();
                    out.insert(MapKey::String("entities".to_string()), Value::Vector(vec![]));
                    out.insert(
                        MapKey::Keyword(Keyword("entities".to_string())),
                        out[&MapKey::String("entities".to_string())].clone(),
                    );
                    out.insert(MapKey::String("tasks".to_string()), Value::Vector(vec![]));
                    out.insert(
                        MapKey::Keyword(Keyword("tasks".to_string())),
                        out[&MapKey::String("tasks".to_string())].clone(),
                    );

                    let mut out_meta = HashMap::new();
                    out_meta.insert(
                        "capability_id".to_string(),
                        Value::String("ccos.chat.transform.extract_entities".to_string()),
                    );
                    out_meta.insert("capability_version".to_string(), Value::String("0.1".to_string()));
                    out_meta.insert(
                        "output_schema".to_string(),
                        Value::String("{entities:[{type:string,value:string}], tasks:string[]}".to_string()),
                    );
                    out_meta.insert(
                        "output_classification".to_string(),
                        Value::String("pii.redacted".to_string()),
                    );
                    out_meta.insert(
                        "policy_pack_version".to_string(),
                        Value::String("chat-mode-v0".to_string()),
                    );
                    out_meta.insert("rule_id".to_string(), Value::String("chat.transform.output".to_string()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "transform.output",
                        out_meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    Ok(attach_label(Value::Map(out), ChatDataLabel::PiiRedacted, None))
                })
            }),
            "low",
            vec!["read".to_string(), "compute".to_string()],
            EffectType::Pure,
        )
        .await?;
    }

    // ---------------------------------------------------------------------
    // Redact message (minimal safe redaction; no raw content output).
    // ---------------------------------------------------------------------
    {
        let quarantine = Arc::clone(&quarantine);
        let chain = Arc::clone(&causal_chain);
        register_native_chat_capability(
            &*marketplace,
            "ccos.chat.transform.redact_message",
            "Redact Message (chat mode)",
            "Read quarantined message by pointer and return pii.redacted output.",
            Arc::new(move |inputs: &Value| {
                let quarantine = Arc::clone(&quarantine);
                let chain = Arc::clone(&chain);
                let inputs = inputs.clone();
                Box::pin(async move {
                    let (pointer_id, justification, session_id, run_id, step_id) =
                        parse_common_transform_inputs(&inputs)?;

                    let mut meta = HashMap::new();
                    meta.insert("pointer_id".to_string(), Value::String(pointer_id.clone()));
                    meta.insert(
                        "capability_id".to_string(),
                        Value::String("ccos.chat.transform.redact_message".to_string()),
                    );
                    meta.insert("capability_version".to_string(), Value::String("0.1".to_string()));
                    meta.insert("justification".to_string(), Value::String(justification));
                    meta.insert("policy_pack_version".to_string(), Value::String("chat-mode-v0".to_string()));
                    meta.insert("rule_id".to_string(), Value::String("chat.quarantine.access".to_string()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "quarantine.access",
                        meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    let bytes = quarantine.get_bytes(&pointer_id)?;
                    let redacted = format!("Redacted message ({} bytes)", bytes.len());

                    let mut out = HashMap::new();
                    out.insert(
                        MapKey::String("redacted_text".to_string()),
                        Value::String(redacted),
                    );
                    out.insert(
                        MapKey::Keyword(Keyword("redacted_text".to_string())),
                        out[&MapKey::String("redacted_text".to_string())].clone(),
                    );
                    out.insert(
                        MapKey::String("redactions".to_string()),
                        Value::Vector(vec![]),
                    );
                    out.insert(
                        MapKey::Keyword(Keyword("redactions".to_string())),
                        out[&MapKey::String("redactions".to_string())].clone(),
                    );

                    let mut out_meta = HashMap::new();
                    out_meta.insert(
                        "capability_id".to_string(),
                        Value::String("ccos.chat.transform.redact_message".to_string()),
                    );
                    out_meta.insert("capability_version".to_string(), Value::String("0.1".to_string()));
                    out_meta.insert(
                        "output_schema".to_string(),
                        Value::String("{redacted_text:string, redactions:[{span:[number,number], type:string}]}".to_string()),
                    );
                    out_meta.insert(
                        "output_classification".to_string(),
                        Value::String("pii.redacted".to_string()),
                    );
                    out_meta.insert(
                        "policy_pack_version".to_string(),
                        Value::String("chat-mode-v0".to_string()),
                    );
                    out_meta.insert("rule_id".to_string(), Value::String("chat.transform.output".to_string()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "transform.output",
                        out_meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    Ok(attach_label(Value::Map(out), ChatDataLabel::PiiRedacted, None))
                })
            }),
            "low",
            vec!["read".to_string(), "compute".to_string()],
            EffectType::Pure,
        )
        .await?;
    }

    // ---------------------------------------------------------------------
    // Redaction verifier (required for public).
    // ---------------------------------------------------------------------
    {
        let chain = Arc::clone(&causal_chain);
        let approval_queue = approval_queue.clone();
        register_native_chat_capability(
            &*marketplace,
            "ccos.chat.transform.verify_redaction",
            "Redaction Verifier (chat mode)",
            "Verify pii.redacted output under constraints and (if approved) produce public text.",
            Arc::new(move |inputs: &Value| {
                let chain = Arc::clone(&chain);
                let approval_queue = approval_queue.clone();
                let inputs = inputs.clone();
                Box::pin(async move {
                    let (text, session_id, run_id, step_id, constraints) =
                        parse_verifier_inputs(&inputs)?;

                    // Require per-run approval to attempt downgrade.
                    if let Some(queue) = approval_queue.as_ref() {
                        let approved = is_chat_public_declassification_approved(queue, &session_id, &run_id).await?;
                        if !approved {
                            return Err(RuntimeError::Generic(
                                "Public declassification not approved for this run".to_string(),
                            ));
                        }
                    } else {
                        return Err(RuntimeError::Generic(
                            "Approval queue not configured for verifier".to_string(),
                        ));
                    }

                    let (ok, issues) = verify_constraints(&text, &constraints);

                    // Audit verifier result (047.transform.output)
                    let mut meta = HashMap::new();
                    meta.insert(
                        "capability_id".to_string(),
                        Value::String("ccos.chat.transform.verify_redaction".to_string()),
                    );
                    meta.insert("capability_version".to_string(), Value::String("0.1".to_string()));
                    meta.insert("output_schema".to_string(), Value::String("{ok:boolean, issues?:string[], text?:string}".to_string()));
                    meta.insert(
                        "output_classification".to_string(),
                        Value::String(if ok { "public" } else { "pii.redacted" }.to_string()),
                    );
                    meta.insert("policy_pack_version".to_string(), Value::String("chat-mode-v0".to_string()));
                    meta.insert("rule_id".to_string(), Value::String("chat.public.declassify".to_string()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "transform.output",
                        meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    let mut out = HashMap::new();
                    out.insert(MapKey::String("ok".to_string()), Value::Boolean(ok));
                    out.insert(
                        MapKey::Keyword(Keyword("ok".to_string())),
                        out[&MapKey::String("ok".to_string())].clone(),
                    );
                    if !issues.is_empty() {
                        out.insert(
                            MapKey::String("issues".to_string()),
                            Value::Vector(issues.iter().cloned().map(Value::String).collect()),
                        );
                        out.insert(
                            MapKey::Keyword(Keyword("issues".to_string())),
                            out[&MapKey::String("issues".to_string())].clone(),
                        );
                    }
                    if ok {
                        out.insert(MapKey::String("text".to_string()), Value::String(text));
                        out.insert(
                            MapKey::Keyword(Keyword("text".to_string())),
                            out[&MapKey::String("text".to_string())].clone(),
                        );
                    }

                    let class = if ok { ChatDataLabel::Public } else { ChatDataLabel::PiiRedacted };
                    Ok(attach_label(Value::Map(out), class, None))
                })
            }),
            "medium",
            vec!["compute".to_string()],
            EffectType::PureProvisional,
        )
        .await?;
    }

    // ---------------------------------------------------------------------
    // Egress gating helper (prepare outbound).
    // ---------------------------------------------------------------------
    {
        let chain = Arc::clone(&causal_chain);
        let approval_queue = approval_queue.clone();
        register_native_chat_capability(
            &*marketplace,
            "ccos.chat.egress.prepare_outbound",
            "Prepare Outbound (chat mode)",
            "Enforce chat-mode egress rules and strip __ccos_meta.",
            Arc::new(move |inputs: &Value| {
                let chain = Arc::clone(&chain);
                let approval_queue = approval_queue.clone();
                let inputs = inputs.clone();
                Box::pin(async move {
                    let (content, session_id, run_id, step_id, policy_pack_version, class_override) =
                        parse_egress_inputs(&inputs)?;

                    let mut label = extract_label(&content);
                    if let Some(override_label) = class_override {
                        label = override_label;
                    }

                    // Deny-by-default: only public may egress without exception.
                    let mut decision = "deny".to_string();
                    let mut rule_id = "chat.egress.default_deny".to_string();
                    let allowed = match label {
                        ChatDataLabel::Public => {
                            decision = "allow".to_string();
                            rule_id = "chat.egress.public".to_string();
                            true
                        }
                        ChatDataLabel::PiiRedacted => {
                            // Redacted egress is deny-by-default; allow only with explicit exception + approval.
                            if let Some(queue) = approval_queue.as_ref() {
                                if is_chat_redacted_egress_approved(queue, &session_id, &run_id).await? {
                                    decision = "allow".to_string();
                                    rule_id = "chat.egress.pii_redacted_exception".to_string();
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };

                    // Audit policy decision (047.policy.decision + 047.egress.attempt)
                    let mut meta = HashMap::new();
                    meta.insert("gate".to_string(), Value::String("egress".to_string()));
                    meta.insert("decision".to_string(), Value::String(decision.clone()));
                    meta.insert("rule_id".to_string(), Value::String(rule_id.clone()));
                    meta.insert(
                        "reason".to_string(),
                        Value::String(format!("payload_classification={}", label.as_str())),
                    );
                    meta.insert("policy_pack_version".to_string(), Value::String(policy_pack_version.clone()));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "policy.decision",
                        meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    let mut meta2 = HashMap::new();
                    meta2.insert("payload_classification".to_string(), Value::String(label.as_str().to_string()));
                    meta2.insert("decision".to_string(), Value::String(decision));
                    meta2.insert("policy_pack_version".to_string(), Value::String(policy_pack_version));
                    meta2.insert("rule_id".to_string(), Value::String(rule_id));
                    record_chat_audit_event(
                        &chain,
                        "chat",
                        "chat",
                        &session_id,
                        &run_id,
                        &step_id,
                        "egress.attempt",
                        meta2,
                        crate::types::ActionType::InternalStep,
                    )?;

                    if !allowed {
                        return Err(RuntimeError::Generic(
                            "Egress denied by chat-mode policy".to_string(),
                        ));
                    }
                    Ok(strip_ccos_meta(&content))
                })
            }),
            "low",
            vec!["compute".to_string()],
            EffectType::Pure,
        )
        .await?;
    }

    // ccos.chat.egress.send_outbound
    if let (Some(connector), Some(handle)) = (connector, connector_handle) {
        let marketplace_cloned = Arc::clone(&marketplace);
        register_native_chat_capability(
            &*marketplace,
            "ccos.chat.egress.send_outbound",
            "Send Outbound (chat mode)",
            "Send content to the outbound channel configured for the connector.",
            Arc::new(move |inputs: &Value| {
                let marketplace = Arc::clone(&marketplace_cloned);
                let connector = Arc::clone(&connector);
                let handle = handle.clone();
                let inputs = inputs.clone();
                Box::pin(async move {
                    // 1. Prepare outbound (enforce policy)
                    let prepared = marketplace
                        .execute_capability("ccos.chat.egress.prepare_outbound", &inputs)
                        .await?;

                    // 2. Extract content and other fields
                    let Value::Map(ref map) = inputs else {
                        return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                    };

                    let channel_id = get_string_arg(map, "channel_id")
                        .unwrap_or_else(|| "default".to_string());
                    let reply_to = get_string_arg(map, "reply_to");
                    
                    let content_text = rtfs_value_to_json(&prepared)?
                        .as_str()
                        .unwrap_or_default()
                        .to_string();

                    if content_text.is_empty() {
                         return Err(RuntimeError::Generic("Prepared outbound content is empty".to_string()));
                    }

                    // 3. Send via connector
                    let outbound = OutboundRequest {
                        channel_id,
                        content: content_text,
                        reply_to,
                        metadata: None,
                    };

                    let result = connector.send(&handle, outbound).await?;
                    if !result.success {
                        return Err(RuntimeError::Generic(format!(
                            "Outbound send failed: {:?}",
                            result.error
                        )));
                    }

                    Ok(Value::Map(HashMap::from([
                        (MapKey::String("success".to_string()), Value::Boolean(true)),
                        (MapKey::String("message_id".to_string()), 
                         Value::String(result.message_id.unwrap_or_default())),
                    ])))
                })
            }),
            "medium",
            vec!["network".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    // -----------------------------------------------------------------
    // Governed instruction resources (URLs/text/files) for autonomy
    // -----------------------------------------------------------------
    {
        let quarantine = Arc::clone(&quarantine);
        let chain = Arc::clone(&causal_chain);
        let store = resource_store.clone();
        let marketplace_for_ingest = Arc::clone(&marketplace);
        register_native_chat_capability(
            &*marketplace,
            "ccos.resource.ingest",
            "Resource / Ingest",
            "Ingest an instruction resource (text or URL) into the governed store. Content is stored in quarantine; metadata is persisted to causal chain. Inputs: {session_id,run_id,step_id, (url|text), content_type?, label?, ttl_seconds?}.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let quarantine = Arc::clone(&quarantine);
                let chain = Arc::clone(&chain);
                let store = store.clone();
                let marketplace = Arc::clone(&marketplace_for_ingest);
                Box::pin(async move {
                    let Value::Map(ref map) = inputs else {
                        return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                    };

                    let get_str = |k: &str| {
                        map.get(&MapKey::String(k.to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword(k.to_string()))))
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string())
                    };
                    let session_id = get_str("session_id")
                        .ok_or_else(|| RuntimeError::Generic("Missing session_id".to_string()))?;
                    let run_id =
                        get_str("run_id").ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;
                    let step_id = get_str("step_id")
                        .ok_or_else(|| RuntimeError::Generic("Missing step_id".to_string()))?;

                    let url = get_str("url");
                    let text = get_str("text").or_else(|| get_str("content"));
                    if url.is_none() && text.is_none() {
                        return Err(RuntimeError::Generic(
                            "ccos.resource.ingest: provide either url or text".to_string(),
                        ));
                    }
                    if url.is_some() && text.is_some() {
                        return Err(RuntimeError::Generic(
                            "ccos.resource.ingest: provide only one of url or text".to_string(),
                        ));
                    }

                    let content_type = get_str("content_type").unwrap_or_else(|| "text/plain".to_string());
                    let label = get_str("label");
                    let ttl_seconds = map
                        .get(&MapKey::String("ttl_seconds".to_string()))
                        .or_else(|| map.get(&MapKey::Keyword(Keyword("ttl_seconds".to_string()))))
                        .and_then(|v| match v {
                            Value::Integer(i) if *i > 0 => Some(*i as i64),
                            _ => None,
                        })
                        .unwrap_or(7 * 24 * 60 * 60); // 7 days default

                    let (source, content) = if let Some(url) = url {
                        if url.starts_with("file://") {
                            let path = url.trim_start_matches("file://");
                            let content = std::fs::read_to_string(path).map_err(|e| {
                                RuntimeError::Generic(format!(
                                    "ccos.resource.ingest: failed to read file {}: {}",
                                    path, e
                                ))
                            })?;
                            (url, content)
                        } else {
                            let mut fetch_inputs = HashMap::new();
                            fetch_inputs.insert(
                                MapKey::String("url".to_string()),
                                Value::String(url.clone()),
                            );
                            fetch_inputs.insert(
                                MapKey::String("method".to_string()),
                                Value::String("GET".to_string()),
                            );
                            let fetched = marketplace
                                .execute_capability("ccos.network.http-fetch", &Value::Map(fetch_inputs))
                                .await?;
                            let Value::Map(out) = fetched else {
                                return Err(RuntimeError::Generic(
                                    "ccos.resource.ingest: http-fetch returned non-map".to_string(),
                                ));
                            };
                            let status = out
                                .get(&MapKey::String("status".to_string()))
                                .and_then(|v| match v {
                                    Value::Integer(i) => Some(*i),
                                    _ => None,
                                })
                                .unwrap_or(0);
                            let body = out
                                .get(&MapKey::String("body".to_string()))
                                .and_then(|v| v.as_string())
                                .unwrap_or("")
                                .to_string();
                            if status >= 400 {
                                return Err(RuntimeError::Generic(format!(
                                    "ccos.resource.ingest: http-fetch failed with status {}",
                                    status
                                )));
                            }
                            (url, body)
                        }
                    } else {
                        ("inline:text".to_string(), text.unwrap_or_default())
                    };

                    let bytes = content.as_bytes().to_vec();
                    let size_bytes = bytes.len() as u64;
                    let source_for_response = source.clone();

                    // Store bytes in quarantine (encrypted-at-rest in FileQuarantineStore).
                    let pointer_id = quarantine.put_bytes(
                        bytes.clone(),
                        chrono::Duration::seconds(ttl_seconds),
                    )?;

                    // Compute sha256 for provenance/audit.
                    let mut hasher = sha2::Sha256::new();
                    sha2::Digest::update(&mut hasher, &bytes);
                    let digest = hasher.finalize();
                    let sha256 = digest.iter().map(|b| format!("{:02x}", b)).collect::<String>();

                    let resource_id = uuid::Uuid::new_v4().to_string();
                    let created_at_ms = chrono::Utc::now().timestamp_millis() as u64;

                    {
                        let mut guard = store
                            .lock()
                            .map_err(|_| RuntimeError::Generic("Failed to lock ResourceStore".to_string()))?;
                        guard.upsert(crate::chat::ResourceRecord {
                            id: resource_id.clone(),
                            pointer_id: pointer_id.clone(),
                            source: source.clone(),
                            content_type: content_type.clone(),
                            sha256: sha256.clone(),
                            size_bytes,
                            created_at_ms,
                            session_id: Some(session_id.clone()),
                            run_id: Some(run_id.clone()),
                            step_id: Some(step_id.clone()),
                            label: label.clone(),
                        });
                    }

                    // Persist minimal provenance to causal chain (no raw content).
                    let plan_id = format!("chat-plan-{}", chrono::Utc::now().timestamp_millis());
                    let intent_id = session_id.clone();
                    let mut meta = HashMap::new();
                    meta.insert("resource_id".to_string(), Value::String(resource_id.clone()));
                    meta.insert("pointer_id".to_string(), Value::String(pointer_id.clone()));
                    meta.insert("source".to_string(), Value::String(source_for_response.clone()));
                    meta.insert("content_type".to_string(), Value::String(content_type.clone()));
                    meta.insert("sha256".to_string(), Value::String(sha256.clone()));
                    meta.insert("size_bytes".to_string(), Value::Integer(size_bytes as i64));
                    meta.insert("created_at_ms".to_string(), Value::Integer(created_at_ms as i64));
                    if let Some(label) = &label {
                        meta.insert("label".to_string(), Value::String(label.clone()));
                    }
                    record_chat_audit_event(
                        &chain,
                        &plan_id,
                        &intent_id,
                        &session_id,
                        &run_id,
                        &step_id,
                        "resource.ingest",
                        meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    let preview: String = content.chars().take(400).collect();
                    let truncated = preview.len() < content.len();

                    Ok(Value::Map(HashMap::from([
                        (MapKey::String("resource_id".to_string()), Value::String(resource_id)),
                        (MapKey::String("pointer_id".to_string()), Value::String(pointer_id)),
                        (MapKey::String("source".to_string()), Value::String(source_for_response)),
                        (MapKey::String("content_type".to_string()), Value::String(content_type)),
                        (MapKey::String("sha256".to_string()), Value::String(sha256)),
                        (MapKey::String("size_bytes".to_string()), Value::Integer(size_bytes as i64)),
                        (MapKey::String("preview".to_string()), Value::String(preview)),
                        (MapKey::String("preview_truncated".to_string()), Value::Boolean(truncated)),
                    ])))
                })
            }),
            "low",
            vec!["storage".to_string(), "network".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    {
        let quarantine = Arc::clone(&quarantine);
        let chain = Arc::clone(&causal_chain);
        let store = resource_store.clone();
        register_native_chat_capability(
            &*marketplace,
            "ccos.resource.get",
            "Resource / Get",
            "Retrieve an ingested instruction resource by resource_id. Inputs: {session_id,run_id,step_id, resource_id, max_len?}. Returns content (possibly truncated) plus metadata.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let quarantine = Arc::clone(&quarantine);
                let chain = Arc::clone(&chain);
                let store = store.clone();
                Box::pin(async move {
                    let Value::Map(ref map) = inputs else {
                        return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                    };
                    let get_str = |k: &str| {
                        map.get(&MapKey::String(k.to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword(k.to_string()))))
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string())
                    };
                    let session_id = get_str("session_id")
                        .ok_or_else(|| RuntimeError::Generic("Missing session_id".to_string()))?;
                    let run_id =
                        get_str("run_id").ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;
                    let step_id = get_str("step_id")
                        .ok_or_else(|| RuntimeError::Generic("Missing step_id".to_string()))?;
                    let resource_id = get_str("resource_id")
                        .ok_or_else(|| RuntimeError::Generic("Missing resource_id".to_string()))?;

                    let max_len = map
                        .get(&MapKey::String("max_len".to_string()))
                        .or_else(|| map.get(&MapKey::Keyword(Keyword("max_len".to_string()))))
                        .and_then(|v| match v {
                            Value::Integer(i) if *i > 0 => Some(*i as usize),
                            _ => None,
                        })
                        .unwrap_or(20_000);

                    let record = {
                        let guard = store
                            .lock()
                            .map_err(|_| RuntimeError::Generic("Failed to lock ResourceStore".to_string()))?;
                        guard
                            .get(&resource_id)
                            .cloned()
                            .ok_or_else(|| RuntimeError::Generic("Resource not found".to_string()))?
                    };

                    let bytes = quarantine.get_bytes(&record.pointer_id)?;
                    let content = String::from_utf8_lossy(&bytes).to_string();
                    let truncated = content.len() > max_len;
                    let content_out = if truncated {
                        content.chars().take(max_len).collect::<String>()
                    } else {
                        content.clone()
                    };

                    // Audit access (no raw content in chain).
                    let plan_id = format!("chat-plan-{}", chrono::Utc::now().timestamp_millis());
                    let intent_id = session_id.clone();
                    let mut meta = HashMap::new();
                    meta.insert("resource_id".to_string(), Value::String(resource_id.clone()));
                    meta.insert("pointer_id".to_string(), Value::String(record.pointer_id.clone()));
                    meta.insert("source".to_string(), Value::String(record.source.clone()));
                    meta.insert("content_type".to_string(), Value::String(record.content_type.clone()));
                    meta.insert("sha256".to_string(), Value::String(record.sha256.clone()));
                    meta.insert("size_bytes".to_string(), Value::Integer(record.size_bytes as i64));
                    meta.insert("max_len".to_string(), Value::Integer(max_len as i64));
                    record_chat_audit_event(
                        &chain,
                        &plan_id,
                        &intent_id,
                        &session_id,
                        &run_id,
                        &step_id,
                        "resource.get",
                        meta,
                        crate::types::ActionType::InternalStep,
                    )?;

                    Ok(Value::Map(HashMap::from([
                        (
                            MapKey::String("resource_id".to_string()),
                            Value::String(resource_id),
                        ),
                        (
                            MapKey::String("content".to_string()),
                            Value::String(content_out),
                        ),
                        (
                            MapKey::String("content_truncated".to_string()),
                            Value::Boolean(truncated),
                        ),
                        (
                            MapKey::String("content_type".to_string()),
                            Value::String(record.content_type),
                        ),
                        (
                            MapKey::String("source".to_string()),
                            Value::String(record.source),
                        ),
                        (
                            MapKey::String("sha256".to_string()),
                            Value::String(record.sha256),
                        ),
                        (
                            MapKey::String("size_bytes".to_string()),
                            Value::Integer(record.size_bytes as i64),
                        ),
                    ])))
                })
            }),
            "low",
            vec!["storage".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    {
        let store = resource_store.clone();
        register_native_chat_capability(
            &*marketplace,
            "ccos.resource.list",
            "Resource / List",
            "List ingested resources for a session or run. Inputs: {session_id? run_id?}. Returns metadata only (no content).",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let store = store.clone();
                Box::pin(async move {
                    let Value::Map(ref map) = inputs else {
                        return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                    };
                    let get_str = |k: &str| {
                        map.get(&MapKey::String(k.to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword(k.to_string()))))
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string())
                    };
                    let session_id = get_str("session_id");
                    let run_id = get_str("run_id");
                    if session_id.is_none() && run_id.is_none() {
                        return Err(RuntimeError::Generic(
                            "ccos.resource.list: provide session_id or run_id".to_string(),
                        ));
                    }

                    let records = {
                        let guard = store
                            .lock()
                            .map_err(|_| RuntimeError::Generic("Failed to lock ResourceStore".to_string()))?;
                        if let Some(rid) = run_id.as_deref() {
                            guard.list_for_run(rid)
                        } else {
                            guard.list_for_session(session_id.as_deref().unwrap_or_default())
                        }
                    };

                    let mut out = Vec::new();
                    for r in records {
                        out.push(Value::Map(HashMap::from([
                            (MapKey::String("resource_id".to_string()), Value::String(r.id.clone())),
                            (MapKey::String("pointer_id".to_string()), Value::String(r.pointer_id.clone())),
                            (MapKey::String("source".to_string()), Value::String(r.source.clone())),
                            (MapKey::String("content_type".to_string()), Value::String(r.content_type.clone())),
                            (MapKey::String("sha256".to_string()), Value::String(r.sha256.clone())),
                            (MapKey::String("size_bytes".to_string()), Value::Integer(r.size_bytes as i64)),
                            (MapKey::String("created_at_ms".to_string()), Value::Integer(r.created_at_ms as i64)),
                        ])));
                    }
                    Ok(Value::Vector(out))
                })
            }),
            "low",
            vec!["storage".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    // -----------------------------------------------------------------
    // Skill Capabilities (for agent to load and execute skills)
    // -----------------------------------------------------------------
    {
        let marketplace_for_skill_load = Arc::clone(&marketplace);
        let approval_queue_for_skill = approval_queue.clone();
        register_native_chat_capability(
            &*marketplace,
            "ccos.skill.load",
            "Load Skill",
            "Load a skill definition from a URL (Markdown/YAML/JSON) and register its capabilities. Returns skill_id, status, and the skill_definition content. Optional input: force=true to bypass URL heuristics.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let marketplace = Arc::clone(&marketplace_for_skill_load);
                let approval_queue = approval_queue_for_skill.clone();
                Box::pin(async move {
                    // Extract URL from inputs
                    let url = if let Value::Map(ref map) = inputs {
                        map.get(&MapKey::String("url".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("url".to_string()))))
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }.ok_or_else(|| RuntimeError::Generic("Missing url parameter".to_string()))?;

                    // === PRE-FLIGHT CHECK: Verify host is in HTTP allowlist or approved ===
                    // This provides a better UX by catching the issue early and creating
                    // an approval request instead of failing with a hard error.
                    if !url.starts_with("file://") {
                        if let Ok(parsed_url) = url.parse::<reqwest::Url>() {
                            let host = parsed_url.host_str().unwrap_or("").to_lowercase();
                            let port = parsed_url.port();
                            
                            log::info!(
                                "[ccos.skill.load] Pre-flight check for URL: {} (host={}, port={:?})",
                                url, host, port
                            );
                            
                            // Check if host is allowed via the capability registry
                            let registry = marketplace.capability_registry.read().await;
                            let host_allowed = registry.is_http_host_allowed(&host);
                            let port_allowed = port.map_or(true, |p| registry.is_http_port_allowed(p));
                            log::info!(
                                "[ccos.skill.load] Registry check: host_allowed={}, port_allowed={}",
                                host_allowed, port_allowed
                            );
                            drop(registry);
                            
                            // Also check if host has been explicitly approved
                            let host_approved = if let Some(queue) = &approval_queue {
                                let approved = queue.is_http_host_approved(&host, port).await.unwrap_or(false);
                                log::info!(
                                    "[ccos.skill.load] Host approval check: approved={}",
                                    approved
                                );
                                approved
                            } else {
                                log::info!("[ccos.skill.load] No approval_queue available");
                                false
                            };
                            
                            log::info!(
                                "[ccos.skill.load] Decision: need_approval={}",
                                (!host_allowed || !port_allowed) && !host_approved
                            );
                            
                            if (!host_allowed || !port_allowed) && !host_approved {
                                log::info!("[ccos.skill.load] Entering approval branch");
                                // Host/port not in allowlist and not approved - create approval request
                                if let Some(queue) = &approval_queue {
                                    log::info!("[ccos.skill.load] Approval queue is available, creating request");
                                    // Clone host for use in multiple places
                                    let host_for_message = host.clone();
                                    
                                    // Extract session_id from inputs if available
                                    let session_id = if let Value::Map(ref map) = inputs {
                                        map.get(&MapKey::String("session_id".to_string()))
                                            .or_else(|| map.get(&MapKey::Keyword(Keyword("session_id".to_string()))))
                                            .and_then(|v| v.as_string())
                                            .map(|s| s.to_string())
                                    } else {
                                        None
                                    };
                                    
                                    let approval_id = queue.add_http_host_approval(
                                        host,
                                        port,
                                        url.clone(),
                                        "session".to_string(),
                                        Some("ccos.skill.load".to_string()),
                                        format!("Skill loading requested access to {}:{}", host_for_message, port.map_or("default".to_string(), |p| p.to_string())),
                                        24, // 24 hour expiry
                                        session_id,
                                    ).await?;
                                    
                                    // Return an error to block plan execution until approval is resolved
                                    // The error includes the approval_id so the user can approve and retry
                                    return Err(RuntimeError::Generic(format!(
                                        "HTTP host '{}' requires approval. Approval ID: {}\n\nUse: /approve {}\n\nOr visit the approval UI to approve this host, then retry the skill loading.",
                                        host_for_message, approval_id, approval_id
                                    )));
                                } else {
                                    // No approval queue configured - return error with helpful message
                                    return Err(RuntimeError::SecurityViolation {
                                        operation: "ccos.skill.load".to_string(),
                                        capability: "ccos.network.http-fetch".to_string(),
                                        context: format!(
                                            "Host '{}' not in HTTP allowlist and no approval queue configured. Add the host to the HTTP allowlist to proceed.",
                                            host
                                        ),
                                    });
                                }
                            }
                        }
                    }
                    // === END PRE-FLIGHT CHECK ===

                    // Optional safety valve: allow callers to override URL heuristics.
                    let force = if let Value::Map(ref map) = inputs {
                        map.get(&MapKey::String("force".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("force".to_string()))))
                            .and_then(|v| match v {
                                Value::Boolean(b) => Some(*b),
                                _ => None,
                            })
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    // Guardrail: skill.load is meant for skill definitions, not arbitrary URLs.
                    // This avoids confusing failures when the user provides, e.g., an X/Twitter tweet URL.
                    if !force && !url_looks_like_skill_definition(&url) {
                        let hint = if url_looks_like_tweet_url(&url) {
                            "This looks like an X/Twitter URL (tweet/profile), not a skill definition. If you're in an onboarding flow, pass this URL to the appropriate skill operation (e.g. verify-human-claim)."
                        } else {
                            "This URL doesn't look like a skill definition. Provide a URL to a skill file (typically .md/.yaml/.yml/.json or a /skill.md endpoint), or set force=true to attempt loading anyway."
                        };
                        return Err(RuntimeError::Generic(format!(
                            "ccos.skill.load: Refusing to load non-skill URL: {}",
                            hint
                        )));
                    }
                    
                    // Fetch the skill definition through governed egress (no direct HTTP here),
                    // then parse it from content. This keeps library code free of network effects.
                    let skill_content = if url.starts_with("file://") {
                        let path = url.trim_start_matches("file://");
                        std::fs::read_to_string(path).map_err(|e| {
                            RuntimeError::Generic(format!(
                                "ccos.skill.load: failed to read file {}: {}",
                                path, e
                            ))
                        })?
                    } else {
                        let mut fetch_inputs = HashMap::new();
                        fetch_inputs.insert(
                            MapKey::String("url".to_string()),
                            Value::String(url.clone()),
                        );
                        fetch_inputs.insert(
                            MapKey::String("method".to_string()),
                            Value::String("GET".to_string()),
                        );
                        let fetched = marketplace
                            .execute_capability("ccos.network.http-fetch", &Value::Map(fetch_inputs))
                            .await?;
                        let Value::Map(map) = fetched else {
                            return Err(RuntimeError::Generic(
                                "ccos.skill.load: http-fetch returned non-map".to_string(),
                            ));
                        };
                        let status = map
                            .get(&MapKey::String("status".to_string()))
                            .and_then(|v| match v {
                                Value::Integer(i) => Some(*i),
                                _ => None,
                            })
                            .unwrap_or(0);
                        let body = map
                            .get(&MapKey::String("body".to_string()))
                            .and_then(|v| v.as_string())
                            .unwrap_or("")
                            .to_string();
                        if status >= 400 {
                            return Err(RuntimeError::Generic(format!(
                                "ccos.skill.load: http-fetch failed with status {}",
                                status
                            )));
                        }
                        body
                    };

                    let loaded_skill =
                        crate::skills::loader::load_skill_from_content(&url, &skill_content)
                            .map_err(|e| RuntimeError::Generic(format!("ccos.skill.load: {}", e)))?;

                    let skill = loaded_skill.skill;
                    let skill_id = skill.id.clone();

                    // Extract base URL from the source URL
                    let base_url = extract_base_url(&loaded_skill.source_url);
                    
                    // Register a capability for each operation found
                    let mut registered_capabilities = Vec::new();
                    for op in &skill.operations {
                        if let Some(endpoint) = &op.endpoint {
                            let capability_id = format!("{}.{}", skill_id, op.name);
                            let full_url = if endpoint.starts_with("http") {
                                endpoint.clone()
                            } else {
                                format!("{}{}", base_url, endpoint)
                            };
                            
                            // Register the capability
                            let manifest = create_http_capability_manifest(
                                &capability_id,
                                &format!("{} - {}", skill_id, op.name),
                                &format!("{} endpoint for {}.{}: {}", op.method.as_deref().unwrap_or("POST"), skill_id, op.name, endpoint),
                                &full_url,
                                op.method.as_deref().unwrap_or("POST"),
                                op.input_schema.clone(),
                            )?;
                            
                            if let Err(e) = marketplace.register_capability_manifest(manifest).await {
                                registered_capabilities.push(format!("{}: failed ({})", capability_id, e));
                            } else {
                                registered_capabilities.push(capability_id.clone());
                            }
                        }
                    }
                    
                    // Build result
                    let mut result_map = HashMap::from([
                        (MapKey::String("skill_id".to_string()), Value::String(skill_id.to_string())),
                        (MapKey::String("status".to_string()), Value::String("loaded".to_string())),
                        (MapKey::String("url".to_string()), Value::String(url)),
                        (MapKey::String("skill_definition".to_string()), Value::String(skill_content)),
                        (MapKey::String("base_url".to_string()), Value::String(base_url)),
                    ]);
                    
                    if !registered_capabilities.is_empty() {
                        let caps: Vec<Value> = registered_capabilities
                            .into_iter()
                            .map(Value::String)
                            .collect();
                        result_map.insert(
                            MapKey::String("registered_capabilities".to_string()),
                            Value::Vector(caps),
                        );
                    } else {
                        return Err(RuntimeError::Generic(
                            "ccos.skill.load: skill registered no capabilities".to_string(),
                        ));
                    }
                    
                    Ok(Value::Map(result_map))
                })
            }),
            "low",
            vec!["network".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    {
        let marketplace_for_skill_execute = Arc::clone(&marketplace);
        register_native_chat_capability(
            &*marketplace,
            "ccos.skill.execute",
            "Execute Skill Operation",
            "Execute an operation from a loaded skill",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let marketplace = Arc::clone(&marketplace_for_skill_execute);
                Box::pin(async move {
                    // Extract skill and operation from inputs
                    let (skill, operation, params) = if let Value::Map(ref map) = inputs {
                        let skill = map.get(&MapKey::String("skill".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("skill".to_string()))))
                            .or_else(|| map.get(&MapKey::String("skill_id".to_string())))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("skill_id".to_string()))))
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string())
                            .ok_or_else(|| RuntimeError::Generic("Missing skill parameter".to_string()))?;
                        
                        let operation = map.get(&MapKey::String("operation".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("operation".to_string()))))
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string())
                            .ok_or_else(|| RuntimeError::Generic("Missing operation parameter".to_string()))?;

                        // Extract parameters (everything except control keys),
                        // and flatten nested "params" / "parameters" / "inputs" maps when present.
                        let mut params_map = HashMap::new();
                        let mut nested_params: Option<HashMap<MapKey, Value>> = None;

                        for (k, v) in map {
                            let key_str = match k {
                                MapKey::String(s) => s.clone(),
                                MapKey::Keyword(Keyword(s)) => s.clone(),
                                _ => continue,
                            };

                            if key_str == "params"
                                || key_str == "parameters"
                                || key_str == "inputs"
                            {
                                if let Value::Map(inner) = v {
                                    nested_params = Some(inner.clone());
                                }
                                continue;
                            }

                            if key_str == "skill"
                                || key_str == "skill_id"
                                || key_str == "operation"
                                || key_str == "session_id"
                                || key_str == "run_id"
                                || key_str == "step_id"
                            {
                                continue;
                            }

                            params_map.insert(k.clone(), v.clone());
                        }

                        if let Some(inner) = nested_params {
                            for (k, v) in inner {
                                params_map.insert(k, v);
                            }
                        }

                        Ok((skill, operation, Value::Map(params_map)))
                    } else {
                        Err(RuntimeError::Generic("Expected map inputs".to_string()))
                    }?;

                    // Normalize names to match registrar logic (lowercase, kebab-case)
                    let normalized_skill = skill.to_lowercase().replace(" ", "-").replace("_", "-");
                    let normalized_op = operation.to_lowercase().replace(" ", "-").replace("_", "-");
                    
                    let capability_id = if normalized_op.contains('.') {
                        normalized_op.clone()
                    } else {
                        format!("{}.{}", normalized_skill, normalized_op)
                    };
                    
                    log::info!("[Gateway] Forwarding ccos.skill.execute to {}", capability_id);

                    // Execute the underlying capability
                    marketplace.execute_capability(&capability_id, &params).await
                })
            }),
            "medium",
            vec!["network".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    // -----------------------------------------------------------------
    // Run Capabilities (for agent to self-schedule)
    // -----------------------------------------------------------------
    if let Some(url_string) = gateway_url {
        let marketplace_for_run = Arc::clone(&marketplace);

        // ccos.run.create
        {
            let url = url_string.clone();
            let secret = internal_secret.clone();
            let marketplace_for_run = Arc::clone(&marketplace_for_run);
            register_native_chat_capability(
                &*marketplace,
                "ccos.run.create",
                "Create/Schedule Run",
                "Create a new run (immediate or scheduled). Inputs: {goal, schedule? (cron), next_run_at? (ISO8601), max_run? (u32), budget?}. Returns {run_id, status}. Use next_run_at for specific times.",
                Arc::new(move |inputs: &Value| {
                    let inputs = inputs.clone();
                    let marketplace = Arc::clone(&marketplace_for_run);
                    let gateway_base = url.clone();
                    let internal_secret = secret.clone();
                    Box::pin(async move {
                        // 1. Extract inputs
                        let Value::Map(ref map) = inputs else {
                            return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                        };

                        let get_str = |k: &str| {
                            map.get(&MapKey::String(k.to_string()))
                                .or_else(|| map.get(&MapKey::Keyword(Keyword(k.to_string()))))
                                .and_then(|v| v.as_string())
                                .map(|s| s.to_string())
                        };
                            let get_int = |k: &str| {
                                map.get(&MapKey::String(k.to_string()))
                                .or_else(|| map.get(&MapKey::Keyword(Keyword(k.to_string()))))
                                .and_then(|v| v.as_integer())
                            };

                        let goal = get_str("goal")
                            .ok_or_else(|| RuntimeError::Generic("Missing goal parameter".to_string()))?;
                        let parent_run_id = get_str("run_id");
                        let schedule = get_str("schedule");
                        let next_run_at = get_str("next_run_at");
                        let max_run = get_int("max_run");
                        let trigger_capability_id = get_str("trigger_capability_id");

                        // 2. Prepare payload for POST /chat/run
                        let session_id = get_str("session_id")
                             .ok_or_else(|| RuntimeError::Generic("Missing session_id (agent should provide its own)".to_string()))?;

                        let mut body_map = HashMap::new();
                        body_map.insert("session_id".to_string(), Value::String(session_id));
                        body_map.insert("goal".to_string(), Value::String(goal));
                        if let Some(run_id) = parent_run_id {
                            body_map.insert("run_id".to_string(), Value::String(run_id));
                        }
                        if let Some(s) = schedule {
                            body_map.insert("schedule".to_string(), Value::String(s));
                        }
                        if let Some(t) = next_run_at {
                            body_map.insert("next_run_at".to_string(), Value::String(t));
                        }
                        if let Some(max) = max_run {
                            body_map.insert("max_run".to_string(), Value::Integer(max));
                        }
                        if let Some(b) = map.get(&MapKey::String("budget".to_string())) {
                             let json_structure = rtfs_value_to_json(b)?;
                             body_map.insert("budget".to_string(), json_to_rtfs_value(&json_structure)?);
                        }
                        if let Some(tcid) = trigger_capability_id {
                            body_map.insert("trigger_capability_id".to_string(), Value::String(tcid));
                        }
                        if let Some(ti) = map.get(&MapKey::String("trigger_inputs".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("trigger_inputs".to_string()))))
                        {
                            body_map.insert("trigger_inputs".to_string(), ti.clone());
                        }

                        let body_json = rtfs_value_to_json(&Value::Map(body_map.into_iter().map(|(k,v)| (MapKey::String(k), v)).collect()))?;
                        let body_str = body_json.to_string();

                        let target_url = format!("{}/chat/run", gateway_base.trim_end_matches('/'));

                        let mut fetch_inputs = HashMap::new();
                        fetch_inputs.insert(MapKey::String("url".to_string()), Value::String(target_url));
                        fetch_inputs.insert(MapKey::String("method".to_string()), Value::String("POST".to_string()));
                        fetch_inputs.insert(MapKey::String("body".to_string()), Value::String(body_str));
                        let mut headers_map = HashMap::from([
                            (MapKey::String("Content-Type".to_string()), Value::String("application/json".to_string())),
                        ]);
                        
                        if let Some(secret) = &internal_secret {
                            headers_map.insert(MapKey::String("X-Internal-Secret".to_string()), Value::String(secret.clone()));
                        }

                        fetch_inputs.insert(MapKey::String("headers".to_string()), Value::Map(headers_map));

                        let fetched = marketplace
                            .execute_capability("ccos.network.http-fetch", &Value::Map(fetch_inputs))
                            .await?;

                        let Value::Map(out) = fetched else {
                             return Err(RuntimeError::Generic("http-fetch returned non-map".to_string()));
                        };

                        let status = out.get(&MapKey::String("status".to_string()))
                            .and_then(|v| match v { Value::Integer(i) => Some(*i), _ => None })
                            .unwrap_or(0);

                        let body = out.get(&MapKey::String("body".to_string()))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Failed to get response body".to_string()))?;
                        
                        if status >= 400 {
                            return Err(RuntimeError::Generic(format!("Create run failed: {} - {}", status, body)));
                        }

                        let json_val: serde_json::Value = serde_json::from_str(body)
                            .map_err(|e| RuntimeError::Generic(format!("Invalid JSON response: {}", e)))?;
                        
                        let run_id = json_val.get("run_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                        // Gateway returns "state" (Debug of RunState enum), not "status"
                        let run_status = json_val.get("state").and_then(|v| v.as_str())
                            .or_else(|| json_val.get("status").and_then(|v| v.as_str()))
                            .unwrap_or("unknown");

                        Ok(Value::Map(HashMap::from([
                            (MapKey::String("run_id".to_string()), Value::String(run_id.to_string())),
                            (MapKey::String("status".to_string()), Value::String(run_status.to_string())),
                        ])))
                    })
                }),
                "medium",
                vec!["network".to_string()],
                EffectType::Effectful,
            )
            .await?;
        }

        // ccos.run.get
        {
            let url = url_string.clone();
            let secret = internal_secret.clone();
            let marketplace_for_run = Arc::clone(&marketplace_for_run);
            register_native_chat_capability(
                &*marketplace,
                "ccos.run.get",
                "Get Run Details",
                "Retrieve status and details of a specific run. Inputs: {run_id}. Returns {run_id, session_id, goal, state, steps_taken, elapsed_secs, budget_max_steps, ...}.",
                Arc::new(move |inputs: &Value| {
                    let inputs = inputs.clone();
                    let marketplace = Arc::clone(&marketplace_for_run);
                    let gateway_base = url.clone();
                    let internal_secret = secret.clone();
                    Box::pin(async move {
                        let Value::Map(ref map) = inputs else {
                            return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                        };

                        let run_id = map.get(&MapKey::String("run_id".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("run_id".to_string()))))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;

                        let target_url = format!("{}/chat/run/{}", gateway_base.trim_end_matches('/'), run_id);
                        let mut fetch_inputs = HashMap::new();
                        fetch_inputs.insert(MapKey::String("url".to_string()), Value::String(target_url));
                        fetch_inputs.insert(MapKey::String("method".to_string()), Value::String("GET".to_string()));
                        
                        let mut headers_map = HashMap::new();
                        if let Some(secret) = &internal_secret {
                            headers_map.insert(MapKey::String("X-Internal-Secret".to_string()), Value::String(secret.clone()));
                        }
                        fetch_inputs.insert(MapKey::String("headers".to_string()), Value::Map(headers_map));

                        let fetched = marketplace.execute_capability("ccos.network.http-fetch", &Value::Map(fetch_inputs)).await?;
                        let Value::Map(out) = fetched else {
                            return Err(RuntimeError::Generic("http-fetch returned non-map".to_string()));
                        };

                        let status = out.get(&MapKey::String("status".to_string()))
                            .and_then(|v| match v { Value::Integer(i) => Some(*i), _ => None })
                            .unwrap_or(0);

                        let body = out.get(&MapKey::String("body".to_string()))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Failed to get response body".to_string()))?;

                        if status >= 400 {
                            return Err(RuntimeError::Generic(format!("Get run failed: {} - {}", status, body)));
                        }

                        let json_val: serde_json::Value = serde_json::from_str(body)
                            .map_err(|e| RuntimeError::Generic(format!("Invalid response: {}", e)))?;
                        
                        Ok(json_to_rtfs_value(&json_val)?)
                    })
                }),
                "medium",
                vec!["network".to_string()],
                EffectType::Effectful,
            )
            .await?;
        }

        // ccos.run.list
        {
            let url = url_string.clone();
            let secret = internal_secret.clone();
            let marketplace_for_run = Arc::clone(&marketplace_for_run);
            register_native_chat_capability(
                &*marketplace,
                "ccos.run.list",
                "List Runs",
                "List all runs for a given session. Inputs: {session_id}. Returns {session_id, runs: [...]}.",
                Arc::new(move |inputs: &Value| {
                    let inputs = inputs.clone();
                    let marketplace = Arc::clone(&marketplace_for_run);
                    let gateway_base = url.clone();
                    let internal_secret = secret.clone();
                    Box::pin(async move {
                        let Value::Map(ref map) = inputs else {
                            return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                        };

                        let session_id = map.get(&MapKey::String("session_id".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("session_id".to_string()))))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Missing session_id".to_string()))?;

                        let target_url = format!("{}/chat/run?session_id={}", gateway_base.trim_end_matches('/'), session_id);
                        let mut fetch_inputs = HashMap::new();
                        fetch_inputs.insert(MapKey::String("url".to_string()), Value::String(target_url));
                        fetch_inputs.insert(MapKey::String("method".to_string()), Value::String("GET".to_string()));
                        
                        let mut headers_map = HashMap::new();
                        if let Some(secret) = &internal_secret {
                            headers_map.insert(MapKey::String("X-Internal-Secret".to_string()), Value::String(secret.clone()));
                        }
                        fetch_inputs.insert(MapKey::String("headers".to_string()), Value::Map(headers_map));

                        let fetched = marketplace.execute_capability("ccos.network.http-fetch", &Value::Map(fetch_inputs)).await?;
                        let Value::Map(out) = fetched else {
                             return Err(RuntimeError::Generic("http-fetch returned non-map".to_string()));
                        };

                        let status = out.get(&MapKey::String("status".to_string()))
                            .and_then(|v| match v { Value::Integer(i) => Some(*i), _ => None })
                            .unwrap_or(0);

                        let body = out.get(&MapKey::String("body".to_string()))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Failed to get response body".to_string()))?;

                        if status >= 400 {
                            return Err(RuntimeError::Generic(format!("List runs failed: {} - {}", status, body)));
                        }

                        let json_val: serde_json::Value = serde_json::from_str(body)
                            .map_err(|e| RuntimeError::Generic(format!("Invalid response: {}", e)))?;
                        
                        Ok(json_to_rtfs_value(&json_val)?)
                    })
                }),
                "medium",
                vec!["network".to_string()],
                EffectType::Effectful,
            )
            .await?;
        }

        // ccos.run.cancel
        {
            let url = url_string.clone();
            let secret = internal_secret.clone();
            let marketplace_for_run = Arc::clone(&marketplace_for_run);
            register_native_chat_capability(
                &*marketplace,
                "ccos.run.cancel",
                "Cancel Run",
                "Cancel an active or paused run. Inputs: {run_id}. Returns {run_id, cancelled, previous_state}.",
                Arc::new(move |inputs: &Value| {
                    let inputs = inputs.clone();
                    let marketplace = Arc::clone(&marketplace_for_run);
                    let gateway_base = url.clone();
                    let internal_secret = secret.clone();
                    Box::pin(async move {
                        let Value::Map(ref map) = inputs else {
                            return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                        };

                        let run_id = map.get(&MapKey::String("run_id".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("run_id".to_string()))))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;

                        let target_url = format!("{}/chat/run/{}/cancel", gateway_base.trim_end_matches('/'), run_id);
                        let mut fetch_inputs = HashMap::new();
                        fetch_inputs.insert(MapKey::String("url".to_string()), Value::String(target_url));
                        fetch_inputs.insert(MapKey::String("method".to_string()), Value::String("POST".to_string()));
                        
                        let mut headers_map = HashMap::new();
                        if let Some(secret) = &internal_secret {
                            headers_map.insert(MapKey::String("X-Internal-Secret".to_string()), Value::String(secret.clone()));
                        }
                        fetch_inputs.insert(MapKey::String("headers".to_string()), Value::Map(headers_map));

                        let fetched = marketplace.execute_capability("ccos.network.http-fetch", &Value::Map(fetch_inputs)).await?;
                        let Value::Map(out) = fetched else {
                             return Err(RuntimeError::Generic("http-fetch returned non-map".to_string()));
                        };

                        let status = out.get(&MapKey::String("status".to_string()))
                            .and_then(|v| match v { Value::Integer(i) => Some(*i), _ => None })
                            .unwrap_or(0);

                        let body = out.get(&MapKey::String("body".to_string()))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Failed to get response body".to_string()))?;

                        if status >= 400 {
                            return Err(RuntimeError::Generic(format!("Cancel run failed: {} - {}", status, body)));
                        }

                        let json_val: serde_json::Value = serde_json::from_str(body)
                            .map_err(|e| RuntimeError::Generic(format!("Invalid response: {}", e)))?;
                        
                        Ok(json_to_rtfs_value(&json_val)?)
                    })
                }),
                "medium",
                vec!["network".to_string()],
                EffectType::Effectful,
            )
            .await?;
        }

        // ccos.run.resume
        {
            let url = url_string.clone();
            let secret = internal_secret.clone();
            let marketplace_for_run = Arc::clone(&marketplace_for_run);
            register_native_chat_capability(
                &*marketplace,
                "ccos.run.resume",
                "Resume Run",
                "Resume a run paused at a checkpoint. Inputs: {run_id}. Returns status 200 on success.",
                Arc::new(move |inputs: &Value| {
                    let inputs = inputs.clone();
                    let marketplace = Arc::clone(&marketplace_for_run);
                    let gateway_base = url.clone();
                    let internal_secret = secret.clone();
                    Box::pin(async move {
                        let Value::Map(ref map) = inputs else {
                            return Err(RuntimeError::Generic("Expected map inputs".to_string()));
                        };

                        let run_id = map.get(&MapKey::String("run_id".to_string()))
                            .or_else(|| map.get(&MapKey::Keyword(Keyword("run_id".to_string()))))
                            .and_then(|v| v.as_string())
                            .ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;

                        let target_url = format!("{}/chat/run/{}/resume", gateway_base.trim_end_matches('/'), run_id);
                        let mut fetch_inputs = HashMap::new();
                        fetch_inputs.insert(MapKey::String("url".to_string()), Value::String(target_url));
                        fetch_inputs.insert(MapKey::String("method".to_string()), Value::String("POST".to_string()));
                        
                        let mut headers_map = HashMap::new();
                        if let Some(secret) = &internal_secret {
                            headers_map.insert(MapKey::String("X-Internal-Secret".to_string()), Value::String(secret.clone()));
                        }
                        fetch_inputs.insert(MapKey::String("headers".to_string()), Value::Map(headers_map));

                        let fetched = marketplace.execute_capability("ccos.network.http-fetch", &Value::Map(fetch_inputs)).await?;
                        let Value::Map(out) = fetched else {
                             return Err(RuntimeError::Generic("http-fetch returned non-map".to_string()));
                        };

                        let body = out.get(&MapKey::String("body".to_string()))
                            .and_then(|v| v.as_string())
                            .unwrap_or("");
                        
                        let status = out.get(&MapKey::String("status".to_string()))
                            .and_then(|v| match v { Value::Integer(i) => Some(*i), _ => None })
                            .unwrap_or(0);

                        if status >= 400 {
                            return Err(RuntimeError::Generic(format!("Resume failed: {} - {}", status, body)));
                        }

                        Ok(Value::Integer(status))
                    })
                }),
                "medium",
                vec!["network".to_string()],
                EffectType::Effectful,
            )
            .await?;
        }
    }
    // -----------------------------------------------------------------
    // Python Code Execution Capability
    // -----------------------------------------------------------------
    {
        use crate::sandbox::bubblewrap::{BubblewrapSandbox, InputFile};
        use crate::sandbox::config::{SandboxConfig, SandboxRuntimeType};
        use crate::sandbox::resources::ResourceLimits;
        use crate::sandbox::DependencyManager;

        // Clone config for capture in closure
        let sandbox_cfg = sandbox_config.clone();
        let approval_queue = approval_queue.clone();
        // Clone marketplace so the sandbox can dispatch ccos_sdk.py CCOS_CALL:: requests.
        let sandbox_marketplace = marketplace.clone();

        register_native_chat_capability(
            &*marketplace,
            "ccos.execute.python",
            "Execute Python Code",
            "Execute Python code in a secure sandboxed environment with file mounting support. Input files are mounted read-only at /workspace/input/. Output files should be written to /workspace/output/. Supports: pandas, numpy, matplotlib, requests. Optional dependencies can be specified for auto-installation (if in allowlist).",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let sandbox_cfg = sandbox_cfg.clone();
                let approval_queue = approval_queue.clone();
                let sandbox_marketplace = sandbox_marketplace.clone();
                Box::pin(async move {
                    // Check if bubblewrap is available (or CCOS_EXECUTE_NO_SANDBOX allows unjailed execution)
                    let bwrap_available = std::process::Command::new("which")
                        .arg("bwrap")
                        .output()
                        .map(|output| output.status.success())
                        .unwrap_or(false);

                    if !bwrap_available && !crate::sandbox::no_sandbox_requested() {
                        return Err(RuntimeError::Generic(
                            "Python execution not available (bubblewrap not installed)".to_string()
                        ));
                    }

                    let sandbox = BubblewrapSandbox::new()
                        .map_err(|e| RuntimeError::Generic(format!("Failed to create sandbox: {}", e)))?;

                    // Parse inputs
                    let map = match &inputs {
                        Value::Map(m) => m,
                        _ => return Err(RuntimeError::Generic("Expected map inputs".to_string())),
                    };

                    // Get code
                    let code = map
                        .get(&MapKey::Keyword(Keyword("code".to_string())))
                        .or_else(|| map.get(&MapKey::String("code".to_string())))
                        .and_then(|v| v.as_string())
                        .ok_or_else(|| RuntimeError::Generic("Missing 'code' parameter".to_string()))?
                        .to_string();

                    // Get input files
                    let mut input_files = Vec::new();
                    if let Some(files_value) = map
                        .get(&MapKey::Keyword(Keyword("input_files".to_string())))
                        .or_else(|| map.get(&MapKey::String("input_files".to_string())))
                    {
                        if let Value::Map(files_map) = files_value {
                            for (key, value) in files_map {
                                let name = match key {
                                    MapKey::String(s) | MapKey::Keyword(Keyword(s)) => s.clone(),
                                    MapKey::Integer(i) => i.to_string(),
                                };
                                let path = value.as_string()
                                    .ok_or_else(|| RuntimeError::Generic(
                                        format!("Invalid path for file '{}'", name)
                                    ))?;
                                input_files.push(InputFile {
                                    name,
                                    host_path: std::path::PathBuf::from(path),
                                });
                            }
                        }
                    }

                    // Validate input files exist
                    for file in &input_files {
                        if !file.host_path.exists() {
                            return Err(RuntimeError::Generic(format!(
                                "Input file '{}' does not exist at path '{}'",
                                file.name,
                                file.host_path.display()
                            )));
                        }
                    }

                    // Get timeout and memory limits
                    let timeout_ms = map
                        .get(&MapKey::Keyword(Keyword("timeout_ms".to_string())))
                        .or_else(|| map.get(&MapKey::String("timeout_ms".to_string())))
                        .and_then(|v| match v {
                            Value::Float(f) => Some(*f as u32),
                            _ => None,
                        })
                        .unwrap_or(30000);

                    let max_memory_mb = map
                        .get(&MapKey::Keyword(Keyword("max_memory_mb".to_string())))
                        .or_else(|| map.get(&MapKey::String("max_memory_mb".to_string())))
                        .and_then(|v| match v {
                            Value::Float(f) => Some(*f as u32),
                            _ => None,
                        })
                        .unwrap_or(512);

                    // Parse dependencies (optional, Phase 2)
                    let mut dependencies = Vec::new();
                    if let Some(deps_value) = map
                        .get(&MapKey::Keyword(Keyword("dependencies".to_string())))
                        .or_else(|| map.get(&MapKey::String("dependencies".to_string())))
                    {
                        if let Value::Vector(deps_vec) = deps_value {
                            for dep in deps_vec {
                                if let Some(dep_str) = dep.as_string() {
                                    dependencies.push(dep_str.to_string());
                                }
                            }
                        }
                    }

                    // Build sandbox execution config (different from config::types::SandboxConfig)
                    let exec_sandbox_config = SandboxConfig {
                        runtime_type: SandboxRuntimeType::Process,
                        capability_id: Some("ccos.execute.python".to_string()),
                        resources: Some(ResourceLimits {
                            memory_mb: max_memory_mb as u64,
                            timeout_ms: timeout_ms as u64,
                            ..Default::default()
                        }),
                        ..Default::default()
                    };

                    // Create dependency manager using captured config
                    let mut effective_sandbox_cfg = sandbox_cfg.clone();
                    
                    // Pre-check for approved packages and add them to temporary auto_approved list
                    if let Some(queue) = &approval_queue {
                        for dep in &dependencies {
                            if queue.is_package_approved(dep, "python").await.unwrap_or(false) {
                                log::debug!("[ccos.execute.python] Package {} is already approved, adding to temporary allowlist", dep);
                                effective_sandbox_cfg.package_allowlist.auto_approved.push(dep.clone());
                            }
                        }
                    }

                    let dep_manager = DependencyManager::new(effective_sandbox_cfg);

                    // Capture session/run context from outer inputs so the dispatcher can
                    // inject them into every SDK-originated ccos.memory.* call, enabling
                    // session tagging (D) for entries written from inside the sandbox.
                    let sdk_session_id = map
                        .get(&MapKey::String("session_id".to_string()))
                        .or_else(|| map.get(&MapKey::Keyword(Keyword("session_id".to_string()))))
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());
                    let sdk_run_id = map
                        .get(&MapKey::String("run_id".to_string()))
                        .or_else(|| map.get(&MapKey::Keyword(Keyword("run_id".to_string()))))
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());

                    // Execute: interactive mode (with ccos_sdk.py IPC) when no deps to install;
                    // fall back to standard mode when dependencies are requested.
                    let result = if dependencies.is_empty() {
                        use std::pin::Pin;
                        use crate::sandbox::bubblewrap::CapabilityDispatcher;
                        use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
                        let mp = sandbox_marketplace.clone();
                        let dispatch_session_id = sdk_session_id.clone();
                        let dispatch_run_id = sdk_run_id.clone();
                        let dispatcher: CapabilityDispatcher = std::sync::Arc::new(
                            move |cap_id: String, mut json_inputs: serde_json::Value| {
                                let mp = mp.clone();
                                let session_id = dispatch_session_id.clone();
                                let run_id = dispatch_run_id.clone();
                                Box::pin(async move {
                                    // Inject session_id and run_id so WM entries are tagged
                                    // correctly (enables C2 cross-run query to find them).
                                    if let Some(obj) = json_inputs.as_object_mut() {
                                        if let Some(ref sid) = session_id {
                                            obj.entry("session_id")
                                                .or_insert_with(|| serde_json::json!(sid));
                                        }
                                        if let Some(ref rid) = run_id {
                                            obj.entry("run_id")
                                                .or_insert_with(|| serde_json::json!(rid));
                                        }
                                    }
                                    let rtfs_inputs = json_to_rtfs_value(&json_inputs)
                                        .map_err(|e| RuntimeError::Generic(
                                            format!("SDK inputs conversion: {}", e),
                                        ))?;
                                    let result = mp.execute_capability(&cap_id, &rtfs_inputs).await?;
                                    rtfs_value_to_json(&result).map_err(|e| RuntimeError::Generic(
                                        format!("SDK result conversion: {}", e),
                                    ))
                                }) as Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, RuntimeError>> + Send>>
                            },
                        );
                        sandbox
                            .execute_python_interactive(&code, &input_files, &exec_sandbox_config, dispatcher)
                            .await
                    } else {
                        sandbox
                            .execute_python(&code, &input_files, &exec_sandbox_config, Some(&dependencies), Some(&dep_manager))
                            .await
                    };

                    let result = match result {
                        Ok(res) => res,
                        Err(e) => {
                            let err_msg = e.to_string();
                            // Check if this is a "requires approval" error
                            if err_msg.contains("requires approval") {
                                // Extract package name: "Package 'mpmath' requires approval..."
                                if let Some(pkg) = err_msg.split('\'').nth(1) {
                                    if let Some(queue) = &approval_queue {
                                        // Extract session_id and run_id from inputs if available
                                        let session_id = map.get(&MapKey::String("session_id".to_string()))
                                            .or_else(|| map.get(&MapKey::Keyword(Keyword("session_id".to_string()))))
                                            .and_then(|v| v.as_string())
                                            .map(|s| s.to_string());

                                        let run_id = map.get(&MapKey::String("run_id".to_string()))
                                            .or_else(|| map.get(&MapKey::Keyword(Keyword("run_id".to_string()))))
                                            .and_then(|v| v.as_string())
                                            .map(|s| s.to_string());

                                        match queue.add_package_approval(
                                            pkg.to_string(), 
                                            "python".to_string(), 
                                            session_id,
                                            run_id,  // Pass run_id for approval resolution
                                        ).await {
                                            Ok(approval_id) => {
                                                return Err(RuntimeError::Generic(format!(
                                                    "Package '{}' requires approval. Approval ID: {}\n\nUse: /approve {}\n\nOr visit the approval UI to approve this package, then retry your request.",
                                                    pkg, approval_id, approval_id
                                                )));
                                            }
                                            Err(ae) => {
                                                log::error!("[ccos.execute.python] Failed to create package approval: {}", ae);
                                            }
                                        }
                                    }
                                }
                            }
                            return Err(RuntimeError::Generic(format!("Execution failed: {}", e)));
                        }
                    };

                    // Check for ModuleNotFoundError in stderr (missing dependency detection)
                    // This handles cases where the code runs but imports a missing module
                    if !result.success {
                        let stderr = &result.stderr;
                        // Pattern: "ModuleNotFoundError: No module named 'X'" or "ImportError: No module named X"
                        if stderr.contains("ModuleNotFoundError") || stderr.contains("ImportError") {
                            // Try to extract the module name
                            let module_name = if let Some(rest) = stderr.split("No module named '").nth(1) {
                                rest.split('\'').next().map(|s| s.to_string())
                            } else if let Some(rest) = stderr.split("No module named \"").nth(1) {
                                rest.split('"').next().map(|s| s.to_string())
                            } else if let Some(rest) = stderr.split("No module named ").nth(1) {
                                // Handle unquoted module names
                                rest.split_whitespace().next()
                                    .map(|s| s.trim_end_matches(',').trim_end_matches('.').to_string())
                            } else {
                                None
                            };

                            if let Some(module) = module_name {
                                log::info!(
                                    "[ccos.execute.python] Detected missing module '{}' in stderr, creating package approval",
                                    module
                                );

                                if let Some(queue) = &approval_queue {
                                    // Extract session_id and run_id from inputs if available
                                    let session_id = map.get(&MapKey::String("session_id".to_string()))
                                        .or_else(|| map.get(&MapKey::Keyword(Keyword("session_id".to_string()))))
                                        .and_then(|v| v.as_string())
                                        .map(|s| s.to_string());

                                    let run_id = map.get(&MapKey::String("run_id".to_string()))
                                        .or_else(|| map.get(&MapKey::Keyword(Keyword("run_id".to_string()))))
                                        .and_then(|v| v.as_string())
                                        .map(|s| s.to_string());

                                    match queue.add_package_approval(
                                        module.clone(),
                                        "python".to_string(),
                                        session_id,
                                        run_id,
                                    ).await {
                                        Ok(approval_id) => {
                                            return Err(RuntimeError::Generic(format!(
                                                "Package '{}' requires approval. Approval ID: {}\n\nUse: /approve {}\n\nOr visit the approval UI to approve this package, then retry your request.",
                                                module, approval_id, approval_id
                                            )));
                                        }
                                        Err(ae) => {
                                            log::error!("[ccos.execute.python] Failed to create package approval for missing module: {}", ae);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Build output
                    let mut output_map = HashMap::new();
                    output_map.insert(
                        MapKey::Keyword(Keyword("success".to_string())),
                        Value::Boolean(result.success),
                    );
                    output_map.insert(
                        MapKey::Keyword(Keyword("stdout".to_string())),
                        Value::String(result.stdout),
                    );
                    output_map.insert(
                        MapKey::Keyword(Keyword("stderr".to_string())),
                        Value::String(result.stderr),
                    );
                    
                    if let Some(exit_code) = result.exit_code {
                        output_map.insert(
                            MapKey::Keyword(Keyword("exit_code".to_string())),
                            Value::Float(exit_code as f64),
                        );
                    } else {
                        output_map.insert(
                            MapKey::Keyword(Keyword("exit_code".to_string())),
                            Value::Nil,
                        );
                    }

                    // Encode output files
                    if !result.output_files.is_empty() {
                        let mut files_map = HashMap::new();
                        for (name, content) in result.output_files {
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&content);
                            files_map.insert(
                                MapKey::String(name),
                                Value::String(encoded),
                            );
                        }
                        output_map.insert(
                            MapKey::Keyword(Keyword("files".to_string())),
                            Value::Map(files_map),
                        );
                    }

                    Ok(Value::Map(output_map))
                })
            }),
            "high",
            vec!["compute".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    // -----------------------------------------------------------------
    // JavaScript Code Execution Capability (Phase 6)
    // -----------------------------------------------------------------
    {
        use crate::sandbox::{BubblewrapSandbox, InputFile, SandboxConfig, SandboxRuntimeType};
        use crate::sandbox::resources::ResourceLimits;
        use crate::sandbox::dependency_manager::DependencyManager;

        let sandbox = Arc::new(BubblewrapSandbox::new()?);
        let marketplace = Arc::clone(&marketplace);
        // Clone config for capture in closure
        let sandbox_cfg = sandbox_config.clone();

        register_native_chat_capability(
            &*marketplace,
            "ccos.execute.javascript",
            "Execute JavaScript Code",
            "Execute Node.js snippets in a secure sandbox. Input should include 'code'. Optional: 'input_files' (map of name to host_path), 'dependencies' (list), 'timeout_ms', 'max_memory_mb'.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let sandbox = Arc::clone(&sandbox);
                let sandbox_cfg = sandbox_cfg.clone();
                Box::pin(async move {
                    let map = match &inputs {
                        Value::Map(m) => m,
                        _ => return Err(RuntimeError::Generic("Input must be a map".to_string())),
                    };

                    let code = map
                        .get(&MapKey::Keyword(Keyword("code".to_string())))
                        .or_else(|| map.get(&MapKey::String("code".to_string())))
                        .and_then(|v| v.as_string())
                        .ok_or_else(|| RuntimeError::Generic("Missing 'code' parameter".to_string()))?
                        .to_string();

                    // Parse input files (Phase 1)
                    let mut input_files = Vec::new();
                    if let Some(files_value) = map
                        .get(&MapKey::Keyword(Keyword("input_files".to_string())))
                        .or_else(|| map.get(&MapKey::String("input_files".to_string())))
                    {
                        if let Value::Map(files_map) = files_value {
                            for (key, value) in files_map {
                                let name = match key {
                                    MapKey::String(s) | MapKey::Keyword(Keyword(s)) => s.clone(),
                                    MapKey::Integer(i) => i.to_string(),
                                };
                                let path = value.as_string()
                                    .ok_or_else(|| RuntimeError::Generic(
                                        format!("Invalid path for file '{}'", name)
                                    ))?;
                                input_files.push(InputFile {
                                    name,
                                    host_path: std::path::PathBuf::from(path),
                                });
                            }
                        }
                    }

                    // Validate input files exist
                    for file in &input_files {
                        if !file.host_path.exists() {
                            return Err(RuntimeError::Generic(format!(
                                "Input file '{}' does not exist at path '{}'",
                                file.name,
                                file.host_path.display()
                            )));
                        }
                    }

                    // Get timeout and memory limits
                    let timeout_ms = map
                        .get(&MapKey::Keyword(Keyword("timeout_ms".to_string())))
                        .or_else(|| map.get(&MapKey::String("timeout_ms".to_string())))
                        .and_then(|v| match v {
                            Value::Float(f) => Some(f.clone() as u32),
                            Value::Integer(i) => Some(i.clone() as u32),
                            _ => None,
                        })
                        .unwrap_or(30000);

                    let max_memory_mb = map
                        .get(&MapKey::Keyword(Keyword("max_memory_mb".to_string())))
                        .or_else(|| map.get(&MapKey::String("max_memory_mb".to_string())))
                        .and_then(|v| match v {
                            Value::Float(f) => Some(f.clone() as u32),
                            Value::Integer(i) => Some(i.clone() as u32),
                            _ => None,
                        })
                        .unwrap_or(512);

                    // Parse dependencies (optional, Phase 6)
                    let mut dependencies = Vec::new();
                    if let Some(deps_value) = map
                        .get(&MapKey::Keyword(Keyword("dependencies".to_string())))
                        .or_else(|| map.get(&MapKey::String("dependencies".to_string())))
                    {
                        if let Value::Vector(deps_vec) = deps_value {
                            for dep in deps_vec {
                                if let Some(dep_str) = dep.as_string() {
                                    dependencies.push(dep_str.to_string());
                                }
                            }
                        }
                    }

                    // Build sandbox execution config
                    let exec_sandbox_config = SandboxConfig {
                        runtime_type: SandboxRuntimeType::Process,
                        capability_id: Some("ccos.execute.javascript".to_string()),
                        resources: Some(ResourceLimits {
                            memory_mb: max_memory_mb as u64,
                            timeout_ms: timeout_ms as u64,
                            ..Default::default()
                        }),
                        ..Default::default()
                    };

                    // Create dependency manager
                    let dep_manager = DependencyManager::new(sandbox_cfg.clone());

                    // Execute with optional dependencies
                    let result = sandbox.execute_javascript(
                        &code, 
                        &input_files, 
                        &exec_sandbox_config,
                        if dependencies.is_empty() { None } else { Some(&dependencies) },
                        Some(&dep_manager)
                    ).await
                        .map_err(|e| RuntimeError::Generic(format!("Execution failed: {}", e)))?;

                    // Build output
                    let mut output_map = HashMap::new();
                    output_map.insert(
                        MapKey::Keyword(Keyword("success".to_string())),
                        Value::Boolean(result.success),
                    );
                    output_map.insert(
                        MapKey::Keyword(Keyword("stdout".to_string())),
                        Value::String(result.stdout),
                    );
                    output_map.insert(
                        MapKey::Keyword(Keyword("stderr".to_string())),
                        Value::String(result.stderr),
                    );
                    
                    if let Some(exit_code) = result.exit_code {
                        output_map.insert(
                            MapKey::Keyword(Keyword("exit_code".to_string())),
                            Value::Float(exit_code as f64),
                        );
                    } else {
                        output_map.insert(
                            MapKey::Keyword(Keyword("exit_code".to_string())),
                            Value::Nil,
                        );
                    }

                    // Encode output files
                    if !result.output_files.is_empty() {
                        let mut files_map = HashMap::new();
                        for (name, content) in result.output_files {
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&content);
                            files_map.insert(
                                MapKey::String(name),
                                Value::String(encoded),
                            );
                        }
                        output_map.insert(
                            MapKey::Keyword(Keyword("files".to_string())),
                            Value::Map(files_map),
                        );
                    }

                    Ok(Value::Map(output_map))
                })
            }),
            "high",
            vec!["compute".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }


    // -----------------------------------------------------------------
    // Specialized Coding Agent Capability (Phase 3)
    // -----------------------------------------------------------------
    {
        use crate::sandbox::coding_agent::{CodingAgent, CodingRequest, CodingConstraints};
        #[allow(unused_imports)]
        use crate::config::types::CodingAgentsConfig;

        // Clone config for capture in closure
        let coding_cfg = coding_agents_config.clone();

        register_native_chat_capability(
            &*marketplace,
            "ccos.delegate.coding_agent",
            "Delegate Code Generation",
            "Delegate code generation to a specialized coding LLM. Returns structured output with code, dependencies, and explanation. Best for complex coding tasks requiring high-quality output.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let coding_cfg = coding_cfg.clone();
                Box::pin(async move {
                    // Parse inputs
                    let map = match &inputs {
                        Value::Map(m) => m,
                        _ => return Err(RuntimeError::Generic("Expected map inputs".to_string())),
                    };

                    // Get task (required)
                    let task = map
                        .get(&MapKey::Keyword(Keyword("task".to_string())))
                        .or_else(|| map.get(&MapKey::String("task".to_string())))
                        .and_then(|v| v.as_string())
                        .ok_or_else(|| RuntimeError::Generic("Missing 'task' parameter".to_string()))?
                        .to_string();

                    // Get language (optional)
                    let language = map
                        .get(&MapKey::Keyword(Keyword("language".to_string())))
                        .or_else(|| map.get(&MapKey::String("language".to_string())))
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());

                    // Get inputs (optional)
                    let mut input_files = Vec::new();
                    if let Some(inputs_value) = map
                        .get(&MapKey::Keyword(Keyword("inputs".to_string())))
                        .or_else(|| map.get(&MapKey::String("inputs".to_string())))
                    {
                        if let Value::Vector(vec) = inputs_value {
                            for item in vec {
                                if let Some(s) = item.as_string() {
                                    input_files.push(s.to_string());
                                }
                            }
                        }
                    }

                    // Get outputs (optional)
                    let mut output_files = Vec::new();
                    if let Some(outputs_value) = map
                        .get(&MapKey::Keyword(Keyword("outputs".to_string())))
                        .or_else(|| map.get(&MapKey::String("outputs".to_string())))
                    {
                        if let Value::Vector(vec) = outputs_value {
                            for item in vec {
                                if let Some(s) = item.as_string() {
                                    output_files.push(s.to_string());
                                }
                            }
                        }
                    }

                    // Get profile (optional)
                    let profile = map
                        .get(&MapKey::Keyword(Keyword("profile".to_string())))
                        .or_else(|| map.get(&MapKey::String("profile".to_string())))
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());

                    // Get constraints (optional)
                    let constraints = map
                        .get(&MapKey::Keyword(Keyword("constraints".to_string())))
                        .or_else(|| map.get(&MapKey::String("constraints".to_string())))
                        .and_then(|v| {
                            if let Value::Map(c) = v {
                                let max_lines = c
                                    .get(&MapKey::Keyword(Keyword("max_lines".to_string())))
                                    .or_else(|| c.get(&MapKey::String("max_lines".to_string())))
                                    .and_then(|v| match v {
                                        Value::Integer(i) => Some(*i as u32),
                                        Value::Float(f) => Some(*f as u32),
                                        _ => None,
                                    });
                                let deps_allowed = c
                                    .get(&MapKey::Keyword(Keyword("dependencies_allowed".to_string())))
                                    .or_else(|| c.get(&MapKey::String("dependencies_allowed".to_string())))
                                    .and_then(|v| match v {
                                        Value::Boolean(b) => Some(*b),
                                        _ => None,
                                    })
                                    .unwrap_or(true);
                                Some(CodingConstraints {
                                    max_lines,
                                    dependencies_allowed: deps_allowed,
                                    timeout_ms: None,
                                })
                            } else {
                                None
                            }
                        });


                    // Build request
                    let request = CodingRequest {
                        task,
                        language,
                        inputs: input_files,
                        outputs: output_files,
                        constraints,
                        profile,
                        prior_attempts: vec![],
                    };

                    // Create coding agent using captured config
                    let agent = CodingAgent::new(coding_cfg.clone());

                    // Generate code
                    let response = agent.generate(&request).await?;

                    // Build output map
                    let mut output_map = HashMap::new();
                    output_map.insert(
                        MapKey::Keyword(Keyword("code".to_string())),
                        Value::String(response.code),
                    );
                    output_map.insert(
                        MapKey::Keyword(Keyword("language".to_string())),
                        Value::String(response.language),
                    );
                    output_map.insert(
                        MapKey::Keyword(Keyword("dependencies".to_string())),
                        Value::Vector(
                            response.dependencies.into_iter().map(Value::String).collect()
                        ),
                    );
                    output_map.insert(
                        MapKey::Keyword(Keyword("explanation".to_string())),
                        Value::String(response.explanation),
                    );
                    if let Some(tests) = response.tests {
                        output_map.insert(
                            MapKey::Keyword(Keyword("tests".to_string())),
                            Value::String(tests),
                        );
                    }

                    Ok(Value::Map(output_map))
                })
            }),
            "medium",
            vec!["llm".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    // -----------------------------------------------------------------
    // Refined Code Execution (Phase 4)
    // -----------------------------------------------------------------
    {
        use crate::sandbox::coding_agent::{CodingAgent, CodingRequest, AttemptContext};
        use crate::sandbox::refiner::{ErrorRefiner, ErrorClass};

        let coding_cfg = coding_agents_config.clone();
        let sandbox_cfg = sandbox_config.clone();
        let marketplace_for_loop = Arc::clone(&marketplace);

        register_native_chat_capability(
            &*marketplace,
            "ccos.code.refined_execute",
            "Refined Code Execution",
            "Generate and execute code with automatic self-correction. If execution fails, the agent will attempt to fix the code based on the error. Supports iterative refinement up to a maximum number of turns.",
            Arc::new(move |inputs: &Value| {
                let coding_cfg = coding_cfg.clone();
                let _sandbox_cfg = sandbox_cfg.clone();
                let marketplace = Arc::clone(&marketplace_for_loop);
                let inputs = inputs.clone();

                Box::pin(async move {
                    // 1. Parse inputs
                    let map = match &inputs {
                        Value::Map(m) => m,
                        _ => return Err(RuntimeError::Generic("Expected map inputs".to_string())),
                    };

                    let task = map
                        .get(&MapKey::Keyword(Keyword("task".to_string())))
                        .or_else(|| map.get(&MapKey::String("task".to_string())))
                        .and_then(|v| v.as_string())
                        .ok_or_else(|| RuntimeError::Generic("Missing 'task' parameter".to_string()))?
                        .to_string();

                    let language = map
                        .get(&MapKey::Keyword(Keyword("language".to_string())))
                        .or_else(|| map.get(&MapKey::String("language".to_string())))
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());

                    let mut output_files = Vec::new();
                    if let Some(outputs_value) = map
                        .get(&MapKey::Keyword(Keyword("outputs".to_string())))
                        .or_else(|| map.get(&MapKey::String("outputs".to_string())))
                    {
                        if let Value::Vector(vec) = outputs_value {
                            for item in vec {
                                if let Some(s) = item.as_string() {
                                    output_files.push(s.to_string());
                                }
                            }
                        }
                    }

                    let mut input_files = Vec::new();
                    if let Some(inputs_value) = map
                        .get(&MapKey::Keyword(Keyword("inputs".to_string())))
                        .or_else(|| map.get(&MapKey::String("inputs".to_string())))
                    {
                        if let Value::Vector(vec) = inputs_value {
                            for item in vec {
                                if let Some(s) = item.as_string() {
                                    input_files.push(s.to_string());
                                }
                            }
                        }
                    }
                    
                    let mut max_turns = coding_cfg.max_coding_turns;
                    if let Some(turns) = map
                        .get(&MapKey::Keyword(Keyword("max_turns".to_string())))
                        .or_else(|| map.get(&MapKey::String("max_turns".to_string())))
                        .and_then(|v| match v {
                            Value::Integer(i) => Some(*i as u32),
                            Value::Float(f) => Some(*f as u32),
                            _ => None,
                        }) {
                        max_turns = turns;
                    }

                    let mut current_attempt = 1;
                    let mut prior_attempts: Vec<AttemptContext> = Vec::new();
                    let mut refinement_history = Vec::new();

                    let coding_agent = CodingAgent::new(coding_cfg.clone());
                    let refiner = ErrorRefiner::new();
                    
                    // We'll track dependencies we discover we need
                    let mut auto_dependencies = Vec::new();

                    while current_attempt <= max_turns {
                        // A. Build prompt adjustments based on last error
                        let mut task_suffix = String::new();
                        if let Some(last) = prior_attempts.last() {
                            // Smart feedback based on error type
                            let classified = refiner.classify_python_error(&last.error);
                            match &classified.class {
                                ErrorClass::MissingDependency(dep) => {
                                    task_suffix = format!("\n\nNote: The previous attempt failed because of a missing module: '{}'. Please ensure it's listed in the dependencies or handled in the code.", dep);
                                    if !auto_dependencies.contains(dep) {
                                        auto_dependencies.push(dep.clone());
                                    }
                                }
                                ErrorClass::Syntax => {
                                    task_suffix = "\n\nNote: The previous attempt had a syntax error. Please double-check indentation and syntax carefully.".to_string();
                                }
                                _ => {}
                            }
                        }

                        // A. Generate/Refine Code
                        let coding_request = CodingRequest {
                            task: format!("{}{}", task, task_suffix),
                            language: language.clone(),
                            inputs: input_files.clone(),
                            outputs: output_files.clone(),
                            constraints: None,
                            profile: None,
                            prior_attempts: prior_attempts.clone(),
                        };

                        let response = coding_agent.generate(&coding_request).await?;

                        // B. Execute Code via ccos.execute.python
                        let mut exec_inputs = HashMap::new();
                        exec_inputs.insert(MapKey::String("code".to_string()), Value::String(response.code.clone()));
                        
                        // Pass auto-discovered dependencies
                        if !auto_dependencies.is_empty() {
                            let mut combined_deps = response.dependencies.clone();
                            for d in &auto_dependencies {
                                if !combined_deps.contains(d) {
                                    combined_deps.push(d.clone());
                                }
                            }
                            let dep_vals = combined_deps.into_iter().map(Value::String).collect();
                            exec_inputs.insert(MapKey::String("dependencies".to_string()), Value::Vector(dep_vals));
                        } else if !response.dependencies.is_empty() {
                            let dep_vals = response.dependencies.iter().map(|d| Value::String(d.clone())).collect();
                            exec_inputs.insert(MapKey::String("dependencies".to_string()), Value::Vector(dep_vals));
                        }

                        let exec_result_val = marketplace.execute_capability("ccos.execute.python", &Value::Map(exec_inputs)).await?;
                        
                        let Value::Map(exec_map) = &exec_result_val else {
                            return Err(RuntimeError::Generic("ccos.execute.python returned non-map".to_string()));
                        };

                        let success = exec_map.get(&MapKey::Keyword(Keyword("success".to_string())))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        // Capture history for this turn
                        let stderr = exec_map.get(&MapKey::Keyword(Keyword("stderr".to_string())))
                            .and_then(|v| v.as_string())
                            .unwrap_or("");
                        
                        let mut history_entry = HashMap::new();
                        history_entry.insert(MapKey::String("attempt".to_string()), Value::Integer(current_attempt as i64));
                        history_entry.insert(MapKey::String("success".to_string()), Value::Boolean(success));
                        history_entry.insert(MapKey::String("code".to_string()), Value::String(response.code.clone()));
                        if !success {
                            history_entry.insert(MapKey::String("error".to_string()), Value::String(stderr.to_string()));
                        }
                        refinement_history.push(Value::Map(history_entry));

                        if success {
                            // Success! Return the response + execution details + history
                            let mut final_map = exec_map.clone();
                            final_map.insert(MapKey::Keyword(Keyword("refinement_cycles".to_string())), Value::Integer(current_attempt as i64));
                            final_map.insert(MapKey::Keyword(Keyword("refinement_history".to_string())), Value::Vector(refinement_history));
                            final_map.insert(MapKey::Keyword(Keyword("final_code".to_string())), Value::String(response.code));
                            final_map.insert(MapKey::Keyword(Keyword("explanation".to_string())), Value::String(response.explanation));
                            return Ok(Value::Map(final_map));
                        }

                        // C. Handle Failure
                        let classified = refiner.classify_python_error(stderr);
                        
                        if current_attempt >= max_turns {
                            // Max turns reached, return the last failure + history
                            let mut final_map = exec_map.clone();
                            final_map.insert(MapKey::Keyword(Keyword("refinement_cycles".to_string())), Value::Integer(current_attempt as i64));
                            final_map.insert(MapKey::Keyword(Keyword("refinement_history".to_string())), Value::Vector(refinement_history));
                            final_map.insert(MapKey::Keyword(Keyword("error_class".to_string())), Value::String(format!("{:?}", classified.class)));
                            return Ok(Value::Map(final_map));
                        }

                        // Update prior attempts for next turn
                        prior_attempts.push(AttemptContext {
                            code: response.code,
                            error: stderr.to_string(), // Keep raw error for summary
                            attempt: current_attempt,
                        });
                        current_attempt += 1;

                        log::info!("Code execution failed ({:?}), starting refinement turn {}/{}", classified.class, current_attempt, max_turns);
                    }

                    Err(RuntimeError::Generic("Max refinement turns exceeded".to_string()))
                })
            }),
            "high",
            vec!["compute".to_string(), "llm".to_string()],
            EffectType::Effectful,
        )
        .await?;
    }

    // -----------------------------------------------------------------
    // RTFS Code Execution Capability (Phase 5)
    // -----------------------------------------------------------------
    {
        use rtfs::runtime::Runtime;

        register_native_chat_capability(
            &*marketplace,
            "ccos.execute.rtfs",
            "Execute RTFS Code",
            "Execute RTFS snippets with access to standard library. Input should be a valid RTFS expression string.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                Box::pin(async move {
                    let map = match &inputs {
                        Value::Map(m) => m,
                        _ => return Err(RuntimeError::Generic("Expected map inputs".to_string())),
                    };

                    let code = map
                        .get(&MapKey::Keyword(Keyword("code".to_string())))
                        .or_else(|| map.get(&MapKey::String("code".to_string())))
                        .and_then(|v| v.as_string())
                        .ok_or_else(|| RuntimeError::Generic("Missing 'code' parameter".to_string()))?
                        .to_string();

                    // Create a runtime and execute (note: Runtime handles registry internally for evaluate_with_stdlib)
                    // We use an empty registry here as evaluate_with_stdlib creates its own loadable stdlib registry.
                    let runtime = Runtime::new_with_tree_walking_strategy(Arc::new(rtfs::runtime::ModuleRegistry::new()));
                    let result = runtime.evaluate_with_stdlib(&code)?;

                    Ok(result)
                })
            }),
            "low",
            vec!["transform".to_string()],
            EffectType::Pure,
        )
        .await?;
    }

    Ok(())
}


/// Extract the base URL (scheme + host + port) from a full URL.
/// E.g., "http://localhost:8765/skills/skill.md" -> "http://localhost:8765"
fn extract_base_url(url: &str) -> String {
    // Parse URL to extract components
    if let Ok(parsed) = url.parse::<reqwest::Url>() {
        let scheme = parsed.scheme();
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port();
        
        if let Some(port) = port {
            format!("{}://{}:{}", scheme, host, port)
        } else {
            format!("{}://{}", scheme, host)
        }
    } else {
        // Fallback: try to extract manually
        url.split('/')
            .take(3)
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn url_looks_like_skill_definition(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();

    // Common explicit skill definition patterns
    if u.contains("/skill.md") {
        return true;
    }

    // Typical file extensions (GitHub raw links, local dev servers, etc.)
    u.ends_with(".md")
        || u.ends_with(".markdown")
        || u.ends_with(".yaml")
        || u.ends_with(".yml")
        || u.ends_with(".json")
}

fn url_looks_like_tweet_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://x.com/")
        || u.starts_with("https://x.com/")
        || u.starts_with("http://twitter.com/")
        || u.starts_with("https://twitter.com/")
        || u.contains("://x.com/")
        || u.contains("://twitter.com/")
}

/// Create an HTTP capability manifest for a skill operation.
fn create_http_capability_manifest(
    id: &str,
    name: &str,
    description: &str,
    endpoint_url: &str,
    method: &str,
    input_schema: Option<rtfs::ast::TypeExpr>,
) -> RuntimeResult<CapabilityManifest> {
    use crate::capability_marketplace::types::HttpCapability;
    
    // Extract skill_id from capability_id (e.g., "moltbook-agent-skill.register-agent" -> "moltbook-agent-skill")
    let skill_id = id.split('.').next().unwrap_or("").to_string();
    let op_name = id.split('.').nth(1).unwrap_or("");
    
    let mut metadata = HashMap::new();
    metadata.insert("name".to_string(), name.to_string());
    metadata.insert("description".to_string(), description.to_string());
    metadata.insert("endpoint".to_string(), endpoint_url.to_string());
    metadata.insert("method".to_string(), method.to_string());
    metadata.insert("security_level".to_string(), "medium".to_string());
    metadata.insert("skill_id".to_string(), skill_id.clone());
    
    // Check if this operation returns a secret (common patterns)
    let returns_secret = op_name.contains("register") 
        || op_name.contains("login") 
        || op_name.contains("auth");
    if returns_secret {
        metadata.insert("returns_secret".to_string(), "true".to_string());
    }
    
    // Create HTTP capability provider with available fields
    let http_cap = HttpCapability {
        base_url: endpoint_url.to_string(),
        auth_token: None,
        timeout_ms: 30000,
    };
    
    Ok(CapabilityManifest {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        provider: ProviderType::Http(http_cap),
        version: "0.1.0".to_string(),
        input_schema,
        output_schema: None,
        attestation: None,
        provenance: None,
        permissions: vec![],
        effects: vec!["network".to_string()],
        metadata: metadata.clone(),
        agent_metadata: None,
        domains: vec!["skill".to_string()],
        categories: vec!["http".to_string()],
        effect_type: EffectType::Effectful,
        approval_status: crate::capability_marketplace::types::ApprovalStatus::Approved,
    })
}

/// Filter tool results before they leave the MCP gateway.
///
/// Deny-by-default for `pii.*` and `secret.*`, allow `public`, and require
/// explicit approval for `pii.redacted` egress.
pub async fn filter_mcp_tool_result(
    chain: &Arc<Mutex<CausalChain>>,
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    plan_id: &str,
    intent_id: &str,
    session_id: &str,
    run_id: &str,
    step_id: &str,
    policy_pack_version: &str,
    result: &Value,
) -> RuntimeResult<Value> {
    let label = extract_label(result);
    let mut decision = "deny".to_string();
    let mut rule_id = "chat.mcp.default_deny".to_string();

    let allowed = match label {
        ChatDataLabel::Public => {
            decision = "allow".to_string();
            rule_id = "chat.mcp.public".to_string();
            true
        }
        ChatDataLabel::PiiRedacted => {
            if let Some(queue) = approval_queue.as_ref() {
                if is_chat_redacted_egress_approved(queue, session_id, run_id).await? {
                    decision = "allow".to_string();
                    rule_id = "chat.mcp.pii_redacted_exception".to_string();
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    };

    let mut meta = HashMap::new();
    meta.insert("gate".to_string(), Value::String("mcp.result".to_string()));
    meta.insert("decision".to_string(), Value::String(decision.clone()));
    meta.insert("rule_id".to_string(), Value::String(rule_id.clone()));
    meta.insert(
        "reason".to_string(),
        Value::String(format!("payload_classification={}", label.as_str())),
    );
    meta.insert(
        "policy_pack_version".to_string(),
        Value::String(policy_pack_version.to_string()),
    );
    record_chat_audit_event(
        chain,
        plan_id,
        intent_id,
        session_id,
        run_id,
        step_id,
        "policy.decision",
        meta,
        crate::types::ActionType::InternalStep,
    )?;

    let mut meta2 = HashMap::new();
    meta2.insert(
        "payload_classification".to_string(),
        Value::String(label.as_str().to_string()),
    );
    meta2.insert("decision".to_string(), Value::String(decision));
    meta2.insert("policy_pack_version".to_string(), Value::String(policy_pack_version.to_string()));
    meta2.insert("rule_id".to_string(), Value::String(rule_id));
    record_chat_audit_event(
        chain,
        plan_id,
        intent_id,
        session_id,
        run_id,
        step_id,
        "egress.attempt",
        meta2,
        crate::types::ActionType::InternalStep,
    )?;

    if !allowed {
        return Err(RuntimeError::Generic(
            "MCP tool result blocked by chat-mode policy".to_string(),
        ));
    }

    Ok(strip_ccos_meta(result))
}

fn get_string_arg(map: &HashMap<MapKey, Value>, key: &str) -> Option<String> {
    map.get(&MapKey::String(key.to_string()))
        .or_else(|| map.get(&MapKey::Keyword(Keyword(key.to_string()))))
        .and_then(|v| v.as_string().map(|s| s.to_string()))
}

fn parse_common_transform_inputs(inputs: &Value) -> RuntimeResult<(String, String, String, String, String)> {
    let Value::Map(map) = inputs else {
        return Err(RuntimeError::Generic("Expected map inputs".to_string()));
    };
    let pointer_id = get_string_arg(map, "content_ref")
        .or_else(|| get_string_arg(map, "pointer_id"))
        .ok_or_else(|| RuntimeError::Generic("Missing content_ref".to_string()))?;
    let justification = get_string_arg(map, "justification")
        .ok_or_else(|| RuntimeError::Generic("Missing justification".to_string()))?;
    let session_id = get_string_arg(map, "session_id")
        .ok_or_else(|| RuntimeError::Generic("Missing session_id".to_string()))?;
    let run_id = get_string_arg(map, "run_id").ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;
    let step_id = get_string_arg(map, "step_id").ok_or_else(|| RuntimeError::Generic("Missing step_id".to_string()))?;
    Ok((pointer_id, justification, session_id, run_id, step_id))
}

fn parse_verifier_inputs(inputs: &Value) -> RuntimeResult<(String, String, String, String, VerifierConstraints)> {
    let Value::Map(map) = inputs else {
        return Err(RuntimeError::Generic("Expected map inputs".to_string()));
    };
    let text = get_string_arg(map, "text")
        .ok_or_else(|| RuntimeError::Generic("Missing text".to_string()))?;
    let session_id = get_string_arg(map, "session_id")
        .ok_or_else(|| RuntimeError::Generic("Missing session_id".to_string()))?;
    let run_id = get_string_arg(map, "run_id").ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;
    let step_id = get_string_arg(map, "step_id").ok_or_else(|| RuntimeError::Generic("Missing step_id".to_string()))?;

    let max_len = map
        .get(&MapKey::String("max_len".to_string()))
        .or_else(|| map.get(&MapKey::Keyword(Keyword("max_len".to_string()))))
        .and_then(|v| v.as_number())
        .map(|n| n as usize)
        .unwrap_or(280);

    Ok((
        text,
        session_id,
        run_id,
        step_id,
        VerifierConstraints {
            max_len,
            forbid_quotes: true,
            forbid_identifiers: true,
        },
    ))
}

fn parse_egress_inputs(
    inputs: &Value,
) -> RuntimeResult<(Value, String, String, String, String, Option<ChatDataLabel>)> {
    let Value::Map(map) = inputs else {
        return Err(RuntimeError::Generic("Expected map inputs".to_string()));
    };
    let content = map
        .get(&MapKey::String("content".to_string()))
        .or_else(|| map.get(&MapKey::Keyword(Keyword("content".to_string()))))
        .ok_or_else(|| RuntimeError::Generic("Missing content".to_string()))?
        .clone();
    let session_id = get_string_arg(map, "session_id")
        .ok_or_else(|| RuntimeError::Generic("Missing session_id".to_string()))?;
    let run_id = get_string_arg(map, "run_id")
        .ok_or_else(|| RuntimeError::Generic("Missing run_id".to_string()))?;
    let step_id = get_string_arg(map, "step_id")
        .ok_or_else(|| RuntimeError::Generic("Missing step_id".to_string()))?;
    let policy_pack_version = get_string_arg(map, "policy_pack_version")
        .unwrap_or_else(|| "chat-mode-v0".to_string());
    let class_override = get_string_arg(map, "content_class").as_deref().and_then(ChatDataLabel::parse);

    Ok((
        content,
        session_id,
        run_id,
        step_id,
        policy_pack_version,
        class_override,
    ))
}

#[derive(Debug, Clone)]
struct VerifierConstraints {
    max_len: usize,
    forbid_quotes: bool,
    forbid_identifiers: bool,
}

fn verify_constraints(text: &str, constraints: &VerifierConstraints) -> (bool, Vec<String>) {
    let mut issues = Vec::new();

    if text.len() > constraints.max_len {
        issues.push(format!("text exceeds max length ({})", constraints.max_len));
    }
    if constraints.forbid_quotes {
        if text.contains('"') || text.contains('“') || text.contains('”') || text.contains('\'') {
            issues.push("text contains quote characters".to_string());
        }
    }
    if constraints.forbid_identifiers {
        // Extremely conservative checks: emails, @handles, long digit sequences.
        match regex::Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}") {
            Ok(email_re) => {
                if email_re.is_match(text) {
                    issues.push("text contains email-like pattern".to_string());
                }
            }
            Err(_) => {
                issues.push("email pattern check failed".to_string());
            }
        }
        if text.contains('@') {
            issues.push("text contains @-handle marker".to_string());
        }
        match regex::Regex::new(r"\d{5,}") {
            Ok(digit_run) => {
                if digit_run.is_match(text) {
                    issues.push("text contains long digit sequence".to_string());
                }
            }
            Err(_) => {
                issues.push("digit pattern check failed".to_string());
            }
        }
    }

    (issues.is_empty(), issues)
}

async fn is_chat_public_declassification_approved(
    queue: &UnifiedApprovalQueue<FileApprovalStorage>,
    session_id: &str,
    run_id: &str,
) -> RuntimeResult<bool> {
    let approvals = queue.list(crate::approval::types::ApprovalFilter::default()).await?;
    for a in approvals {
        if !a.status.is_approved() {
            continue;
        }
        if let ApprovalCategory::ChatPublicDeclassification { session_id: sid, run_id: rid, .. } =
            &a.category
        {
            if sid == session_id && rid == run_id {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn is_chat_redacted_egress_approved(
    queue: &UnifiedApprovalQueue<FileApprovalStorage>,
    session_id: &str,
    run_id: &str,
) -> RuntimeResult<bool> {
    let approvals = queue.list(crate::approval::types::ApprovalFilter::default()).await?;
    for a in approvals {
        if !a.status.is_approved() {
            continue;
        }
        if let ApprovalCategory::ChatPolicyException { kind, session_id: sid, run_id: rid, .. } =
            &a.category
        {
            if kind == "egress.pii_redacted" && sid == session_id && rid == run_id {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Convenience helpers to create approval requests for chat mode.
pub async fn request_chat_policy_exception(
    queue: &UnifiedApprovalQueue<FileApprovalStorage>,
    kind: &str,
    session_id: &str,
    run_id: &str,
    context: String,
) -> RuntimeResult<String> {
    let request = crate::approval::types::ApprovalRequest::new(
        ApprovalCategory::ChatPolicyException {
            kind: kind.to_string(),
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
        },
        RiskAssessment {
            level: RiskLevel::High,
            reasons: vec![format!("Chat policy exception requested: {}", kind)],
        },
        24,
        Some(context),
    );
    queue.add(request).await
}

pub async fn request_chat_public_declassification(
    queue: &UnifiedApprovalQueue<FileApprovalStorage>,
    session_id: &str,
    run_id: &str,
    transform_capability_id: &str,
    verifier_capability_id: &str,
    constraints: &str,
    context: String,
) -> RuntimeResult<String> {
    let request = crate::approval::types::ApprovalRequest::new(
        ApprovalCategory::ChatPublicDeclassification {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            transform_capability_id: transform_capability_id.to_string(),
            verifier_capability_id: verifier_capability_id.to_string(),
            constraints: constraints.to_string(),
        },
        RiskAssessment {
            level: RiskLevel::High,
            reasons: vec!["Chat public declassification requested".to_string()],
        },
        24,
        Some(context),
    );
    queue.add(request).await
}

/// Approve a chat-related approval request (test helper / CLI may wrap this).
pub async fn approve_request(
    queue: &UnifiedApprovalQueue<FileApprovalStorage>,
    approval_id: &str,
    by: ApprovalAuthority,
    reason: Option<String>,
) -> RuntimeResult<()> {
    queue.approve(approval_id, by, reason).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::registry::CapabilityRegistry;
    use crate::capability_marketplace::CapabilityMarketplace;
    use crate::chat::quarantine::InMemoryQuarantineStore;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_ccos_run_create_capability() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let quarantine = Arc::new(InMemoryQuarantineStore::new());
        let chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
        let resource_store = new_shared_resource_store();
        
        // Mock ccos.network.http-fetch to return a simulated response
        {
            let marketplace_clone = marketplace.clone();
            let mut manifest = CapabilityManifest::new(
                "ccos.network.http-fetch".to_string(),
                "HTTP Fetch".to_string(),
                "Mock HTTP fetch".to_string(),
                ProviderType::Native(NativeCapability {
                    handler: Arc::new(|inputs: &Value| {
                         let inputs = inputs.clone();
                         Box::pin(async move {
                             let Value::Map(map) = inputs else { panic!("Invalid inputs") };
                             let url = map.get(&MapKey::String("url".to_string())).unwrap().as_string().unwrap();
                             assert!(url.contains("/chat/run"));
                             
                             let body_str = map.get(&MapKey::String("body".to_string())).unwrap().as_string().unwrap();
                             assert!(body_str.contains("goal"));

                             let headers = map.get(&MapKey::String("headers".to_string())).unwrap();
                             let Value::Map(h_map) = headers else { panic!("Headers not a map") };
                             let secret = h_map.get(&MapKey::String("X-Internal-Secret".to_string())).unwrap().as_string().unwrap();
                             assert_eq!(secret, "mock-secret");
                             
                             // Gateway serialises RunState via Debug, so "state" not "status"
                             let response_body = serde_json::json!({
                                 "run_id": "run-123",
                                 "state": "Scheduled"
                             }).to_string();
                             
                             Ok(Value::Map(HashMap::from([
                                 (MapKey::String("status".to_string()), Value::Integer(200)),
                                 (MapKey::String("body".to_string()), Value::String(response_body)),
                             ])))
                         })
                    }),
                    security_level: "low".to_string(),
                    metadata: HashMap::new(),
                }),
                "1.0.0".to_string(),
            );
            // This is an internal mock used by the test; bypass governance gating.
            manifest.approval_status =
                crate::capability_marketplace::types::ApprovalStatus::AutoApproved;
            marketplace_clone.register_capability_manifest(manifest).await.unwrap();
        }

        register_chat_capabilities(
            marketplace.clone(),
            quarantine,
            chain,
            None,
            resource_store,
            None,
            None,
            Some("http://localhost:9999".to_string()),
            Some("mock-secret".to_string()),
            crate::config::types::SandboxConfig::default(),
            crate::config::types::CodingAgentsConfig::default(),
        ).await.unwrap();

        let inputs = Value::Map(HashMap::from([
            (MapKey::String("goal".to_string()), Value::String("Take over the world".to_string())),
            (MapKey::String("session_id".to_string()), Value::String("session-1".to_string())),
            (MapKey::String("schedule".to_string()), Value::String("in 10s".to_string())),
        ]));

        let result = marketplace.execute_capability("ccos.run.create", &inputs).await.unwrap();
        
        let Value::Map(out) = result else { panic!("Expected map result") };
        assert_eq!(out.get(&MapKey::String("run_id".to_string())).unwrap().as_string().unwrap(), "run-123");
        // Must return the "state" field value, not "unknown"
        assert_eq!(out.get(&MapKey::String("status".to_string())).unwrap().as_string().unwrap(), "Scheduled");
    }

    /// Regression test: trigger_capability_id and trigger_inputs MUST be forwarded
    /// in the POST /chat/run payload for stateful recurring Python capability runs.
    /// Without them the gateway spawns an LLM agent instead of executing the code
    /// directly, and state continuity is lost.
    #[tokio::test]
    async fn test_ccos_run_create_forwards_trigger_fields() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let quarantine = Arc::new(InMemoryQuarantineStore::new());
        let chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
        let resource_store = new_shared_resource_store();

        let trigger_seen = Arc::new(AtomicBool::new(false));
        let inputs_seen = Arc::new(AtomicBool::new(false));

        {
            let trigger_seen = Arc::clone(&trigger_seen);
            let inputs_seen = Arc::clone(&inputs_seen);
            let marketplace_clone = marketplace.clone();
            let mut manifest = CapabilityManifest::new(
                "ccos.network.http-fetch".to_string(),
                "HTTP Fetch".to_string(),
                "Mock HTTP fetch".to_string(),
                ProviderType::Native(NativeCapability {
                    handler: Arc::new(move |inputs: &Value| {
                        let inputs = inputs.clone();
                        let trigger_seen = Arc::clone(&trigger_seen);
                        let inputs_seen = Arc::clone(&inputs_seen);
                        Box::pin(async move {
                            let Value::Map(map) = inputs else { panic!("Expected map") };
                            let body_str = map
                                .get(&MapKey::String("body".to_string()))
                                .unwrap()
                                .as_string()
                                .unwrap();
                            let body: serde_json::Value = serde_json::from_str(&body_str).unwrap();

                            // Assert trigger_capability_id is present
                            if body.get("trigger_capability_id").and_then(|v| v.as_str())
                                == Some("ccos.execute.python")
                            {
                                trigger_seen.store(true, Ordering::SeqCst);
                            }
                            // Assert trigger_inputs is present and contains the code key
                            if let Some(ti) = body.get("trigger_inputs") {
                                if ti.get("code").is_some() {
                                    inputs_seen.store(true, Ordering::SeqCst);
                                }
                            }

                            let response_body = serde_json::json!({
                                "run_id": "run-trigger-test",
                                "state": "Scheduled"
                            })
                            .to_string();
                            Ok(Value::Map(HashMap::from([
                                (MapKey::String("status".to_string()), Value::Integer(200)),
                                (MapKey::String("body".to_string()), Value::String(response_body)),
                            ])))
                        })
                    }),
                    security_level: "low".to_string(),
                    metadata: HashMap::new(),
                }),
                "1.0.0".to_string(),
            );
            manifest.approval_status =
                crate::capability_marketplace::types::ApprovalStatus::AutoApproved;
            marketplace_clone.register_capability_manifest(manifest).await.unwrap();
        }

        register_chat_capabilities(
            marketplace.clone(),
            quarantine,
            chain,
            None,
            resource_store,
            None,
            None,
            Some("http://localhost:9999".to_string()),
            Some("mock-secret".to_string()),
            crate::config::types::SandboxConfig::default(),
            crate::config::types::CodingAgentsConfig::default(),
        )
        .await
        .unwrap();

        let trigger_inputs_val = Value::Map(HashMap::from([
            (
                MapKey::String("code".to_string()),
                Value::String("import ccos_sdk; print('fib')".to_string()),
            ),
        ]));

        let inputs = Value::Map(HashMap::from([
            (MapKey::String("goal".to_string()), Value::String("Compute Fibonacci".to_string())),
            (MapKey::String("session_id".to_string()), Value::String("session-fib".to_string())),
            (MapKey::String("schedule".to_string()), Value::String("every 15s".to_string())),
            (
                MapKey::String("trigger_capability_id".to_string()),
                Value::String("ccos.execute.python".to_string()),
            ),
            (MapKey::String("trigger_inputs".to_string()), trigger_inputs_val),
        ]));

        let result = marketplace
            .execute_capability("ccos.run.create", &inputs)
            .await
            .unwrap();

        let Value::Map(out) = result else { panic!("Expected map result") };
        assert_eq!(
            out.get(&MapKey::String("run_id".to_string()))
                .unwrap()
                .as_string()
                .unwrap(),
            "run-trigger-test"
        );
        assert_eq!(
            out.get(&MapKey::String("status".to_string()))
                .unwrap()
                .as_string()
                .unwrap(),
            "Scheduled",
            "status should be 'Scheduled' from the 'state' field, not 'unknown'"
        );
        assert!(
            trigger_seen.load(Ordering::SeqCst),
            "trigger_capability_id was not forwarded in the POST body"
        );
        assert!(
            inputs_seen.load(Ordering::SeqCst),
            "trigger_inputs was not forwarded in the POST body"
        );
    }

    #[tokio::test]
    async fn test_python_package_approval() {
        use crate::approval::storage_file::FileApprovalStorage;
        use crate::approval::unified_queue::UnifiedApprovalQueue;
        use crate::capability_marketplace::CapabilityMarketplace;
        use crate::chat::register_chat_capabilities;
        use crate::capabilities::registry::CapabilityRegistry;
        use crate::causal_chain::CausalChain;
        use crate::chat::quarantine::InMemoryQuarantineStore;
        use crate::chat::resource::new_shared_resource_store;
        use tempfile::tempdir;
        
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
        let quarantine = Arc::new(InMemoryQuarantineStore::new());
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
        let resource_store = new_shared_resource_store();
        
        let temp_dir = tempdir().unwrap();
        let storage = Arc::new(FileApprovalStorage::new(temp_dir.path().to_path_buf()).unwrap());
        let approval_queue = UnifiedApprovalQueue::new(storage);

        register_chat_capabilities(
            marketplace.clone(),
            quarantine,
            causal_chain,
            Some(approval_queue.clone()),
            resource_store,
            None,
            None,
            None,
            None,
            crate::config::types::SandboxConfig::default(),
            crate::config::types::CodingAgentsConfig::default(),
        ).await.unwrap();

        // ccos.execute.python with unapproved package 'mpmath'
        let inputs = Value::Map(HashMap::from([
            (MapKey::String("code".to_string()), Value::String("import mpmath; print(mpmath.pi)".to_string())),
            (MapKey::String("dependencies".to_string()), Value::Vector(vec![Value::String("mpmath".to_string())])),
        ]));

        let result = marketplace.execute_capability("ccos.execute.python", &inputs).await;
        
        // It should return an error with a retry hint or containing "requires approval"
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.err().unwrap());
        assert!(err_msg.contains("requires approval") || err_msg.contains("hint"));
        
        // Verify that a package approval request was created in the queue
        let pending = approval_queue.list_pending_packages().await.unwrap();
        assert!(!pending.is_empty());
        
        if let crate::approval::types::ApprovalCategory::PackageApproval { package, .. } = &pending[0].category {
            assert_eq!(package, "mpmath");
        } else {
            panic!("Expected PackageApproval category");
        }
    }
    #[tokio::test]
    async fn test_ccos_run_management_capabilities() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let quarantine = Arc::new(InMemoryQuarantineStore::new());
        let chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
        let resource_store = new_shared_resource_store();
        
        // Mock ccos.network.http-fetch to handle various run management calls
        {
            let marketplace_clone = marketplace.clone();
            let mut manifest = CapabilityManifest::new(
                "ccos.network.http-fetch".to_string(),
                "HTTP Fetch".to_string(),
                "Mock HTTP fetch".to_string(),
                ProviderType::Native(NativeCapability {
                    handler: Arc::new(|inputs: &Value| {
                         let inputs = inputs.clone();
                         Box::pin(async move {
                             let Value::Map(map) = inputs else { panic!("Invalid inputs") };
                             let url = map.get(&MapKey::String("url".to_string())).unwrap().as_string().unwrap();
                             let method = map.get(&MapKey::String("method".to_string())).unwrap().as_string().unwrap();

                             let headers = map.get(&MapKey::String("headers".to_string())).unwrap();
                             let Value::Map(h_map) = headers else { panic!("Headers not a map") };
                             let secret = h_map.get(&MapKey::String("X-Internal-Secret".to_string())).unwrap().as_string().unwrap();
                             assert_eq!(secret, "mock-secret");

                             let (response_body, status) = if url.contains("/chat/run/run-unauthorized") {
                                 ("".to_string(), 401)
                             } else if url.contains("/chat/run/run-456/cancel") {
                                 assert_eq!(method, "POST");
                                 (serde_json::json!({ "run_id": "run-456", "cancelled": true, "previous_state": "running" }).to_string(), 200)
                             } else if url.contains("/chat/run/run-456/resume") {
                                 assert_eq!(method, "POST");
                                 ("".to_string(), 200)
                             } else if url.contains("/chat/run/run-456") {
                                 assert_eq!(method, "GET");
                                 (serde_json::json!({ "run_id": "run-456", "status": "running", "goal": "test goal" }).to_string(), 200)
                             } else if url.contains("/chat/run?session_id=session-1") {
                                 assert_eq!(method, "GET");
                                 (serde_json::json!({ "session_id": "session-1", "runs": [{"run_id": "run-456"}] }).to_string(), 200)
                             } else if url.contains("/chat/run/chat-run-123") {
                                 assert_eq!(method, "GET");
                                 (serde_json::json!({ "run_id": "chat-run-123", "status": "running", "goal": "test goal" }).to_string(), 200)
                             } else {
                                 panic!("Unexpected URL: {}", url);
                             };
                             
                             Ok(Value::Map(HashMap::from([
                                 (MapKey::String("status".to_string()), Value::Integer(status)),
                                 (MapKey::String("body".to_string()), Value::String(response_body)),
                             ])))
                         })
                    }),
                    security_level: "low".to_string(),
                    metadata: HashMap::new(),
                }),
                "1.0.0".to_string(),
            );
            manifest.approval_status = crate::capability_marketplace::types::ApprovalStatus::AutoApproved;
            marketplace_clone.register_capability_manifest(manifest).await.unwrap();
        }

        register_chat_capabilities(
            marketplace.clone(),
            quarantine,
            chain,
            None,
            resource_store,
            None,
            None,
            Some("http://localhost:9999".to_string()),
            Some("mock-secret".to_string()),
            crate::config::types::SandboxConfig::default(),
            crate::config::types::CodingAgentsConfig::default(),
        ).await.unwrap();

        // 1. Test ccos.run.get
        let get_inputs = Value::Map(HashMap::from([
            (MapKey::String("run_id".to_string()), Value::String("run-456".to_string())),
        ]));
        let get_res = marketplace.execute_capability("ccos.run.get", &get_inputs).await.unwrap();
        let Value::Map(get_map) = get_res else { panic!("Expected map") };
        assert_eq!(get_map.get(&MapKey::String("run_id".to_string())).unwrap().as_string().unwrap(), "run-456");

        // 2. Test ccos.run.list
        let list_inputs = Value::Map(HashMap::from([
            (MapKey::String("session_id".to_string()), Value::String("session-1".to_string())),
        ]));
        let list_res = marketplace.execute_capability("ccos.run.list", &list_inputs).await.unwrap();
        let Value::Map(list_map) = list_res else { panic!("Expected map") };
        assert_eq!(list_map.get(&MapKey::String("session_id".to_string())).unwrap().as_string().unwrap(), "session-1");

        // 3. Test ccos.run.cancel
        let cancel_inputs = Value::Map(HashMap::from([
            (MapKey::String("run_id".to_string()), Value::String("run-456".to_string())),
        ]));
        let cancel_res = marketplace.execute_capability("ccos.run.cancel", &cancel_inputs).await.unwrap();
        let Value::Map(cancel_map) = cancel_res else { panic!("Expected map") };
        assert_eq!(cancel_map.get(&MapKey::String("cancelled".to_string())).unwrap().as_bool().unwrap(), true);

        // 4. Test ccos.run.resume
        let resume_inputs = Value::Map(HashMap::from([
            (MapKey::String("run_id".to_string()), Value::String("run-456".to_string())),
        ]));
        let resume_res = marketplace.execute_capability("ccos.run.resume", &resume_inputs).await.unwrap();
        assert_eq!(resume_res.as_number().unwrap(), 200.0);

        // 5. Test error case: 401 Unauthorized with empty body (reported EOF error)
        let get_inputs_err = Value::Map(HashMap::from([
            (MapKey::String("run_id".to_string()), Value::String("run-unauthorized".to_string())),
        ]));
        let err_res = marketplace.execute_capability("ccos.run.get", &get_inputs_err).await;
        assert!(err_res.is_err());
        let err_msg = format!("{:?}", err_res.err().unwrap());
        assert!(err_msg.contains("failed: 401"));
        assert!(!err_msg.contains("EOF"));

        // 6. Test ephemeral run retrieval (verifying 404 fix)
        let get_inputs_eph = Value::Map(HashMap::from([
            (MapKey::String("run_id".to_string()), Value::String("chat-run-123".to_string())),
        ]));
        let eph_res = marketplace.execute_capability("ccos.run.get", &get_inputs_eph).await.unwrap();
        let Value::Map(eph_map) = eph_res else { panic!("Expected map") };
        assert_eq!(eph_map.get(&MapKey::String("run_id".to_string())).unwrap().as_string().unwrap(), "chat-run-123");
    }
}
