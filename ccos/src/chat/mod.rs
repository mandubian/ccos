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
pub mod checkpoint;
pub mod connector;
pub mod gateway;
pub mod quarantine;
pub mod predicate;
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
pub use run::{BudgetContext, Run, RunState, RunStore, SharedRunStore, new_shared_run_store};
pub use scheduler::Scheduler;
pub use session::{ChatMessage, SessionRegistry};
pub use spawner::{AgentSpawner, SpawnConfig, SpawnerFactory};

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


/// Records a chat audit event into the causal chain as an `InternalStep`.
///
/// This is the Phase 0 minimal enforcement backing `047-chat-audit-events.md`.
pub fn record_chat_audit_event(
    chain: &Arc<Mutex<CausalChain>>,
    plan_id: &str,
    intent_id: &str,
    session_id: &str,
    run_id: &str,
    step_id: &str,
    event_type: &str,
    mut metadata: HashMap<String, Value>,
) -> RuntimeResult<()> {
    metadata.insert("event_type".to_string(), Value::String(event_type.to_string()));
    metadata.insert("session_id".to_string(), Value::String(session_id.to_string()));
    metadata.insert("run_id".to_string(), Value::String(run_id.to_string()));
    metadata.insert("step_id".to_string(), Value::String(step_id.to_string()));

    let action = Action {
        action_id: uuid::Uuid::new_v4().to_string(),
        parent_action_id: None,
        session_id: Some(session_id.to_string()),
        plan_id: plan_id.to_string(),
        intent_id: intent_id.to_string(),
        action_type: ActionType::InternalStep,
        function_name: Some(format!("chat.audit.{}", event_type)),
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
        register_native_chat_capability(
            &*marketplace,
            "ccos.skill.load",
            "Load Skill",
            "Load a skill definition from a URL (Markdown/YAML/JSON) and register its capabilities. Returns skill_id, status, and the skill_definition content. Optional input: force=true to bypass URL heuristics.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let marketplace = Arc::clone(&marketplace_for_skill_load);
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
                        // and flatten a nested "params" map when present.
                        let mut params_map = HashMap::new();
                        let mut nested_params: Option<HashMap<MapKey, Value>> = None;

                        for (k, v) in map {
                            let key_str = match k {
                                MapKey::String(s) => s.clone(),
                                MapKey::Keyword(Keyword(s)) => s.clone(),
                                _ => continue,
                            };

                            if key_str == "params" {
                                if let Value::Map(inner) = v {
                                    nested_params = Some(inner.clone());
                                }
                                continue;
                            }

                            if key_str == "skill"
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
                    
                    let capability_id = format!("{}.{}", normalized_skill, normalized_op);
                    
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
        register_native_chat_capability(
            &*marketplace,
            "ccos.run.create",
            "Create/Schedule Run",
            "Create a new run (immediate or scheduled). Inputs: {goal, schedule? (cron), next_run_at? (ISO8601), budget?}. Returns {run_id, status}. Use next_run_at for specific times.",
            Arc::new(move |inputs: &Value| {
                let inputs = inputs.clone();
                let marketplace = Arc::clone(&marketplace_for_run);
                let gateway_base = url_string.clone();
                let internal_secret = internal_secret.clone();
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

                    let goal = get_str("goal")
                        .ok_or_else(|| RuntimeError::Generic("Missing goal parameter".to_string()))?;
                    let schedule = get_str("schedule");
                    let next_run_at = get_str("next_run_at");

                    // 2. Prepare payload for POST /chat/run
                    // Note: We need the current session_id. The agent calling this is in a session.
                    // We can either require session_id in inputs, or let the caller infer it.
                    // For now, require it in inputs as the agent usually knows its session.
                    let session_id = get_str("session_id")
                         .ok_or_else(|| RuntimeError::Generic("Missing session_id (agent should provide its own)".to_string()))?;

                    // Construct body manually
                    let mut body_map = HashMap::new();
                    body_map.insert("session_id".to_string(), Value::String(session_id));
                    body_map.insert("goal".to_string(), Value::String(goal));
                    if let Some(s) = schedule {
                        body_map.insert("schedule".to_string(), Value::String(s));
                    }
                    if let Some(t) = next_run_at {
                        body_map.insert("next_run_at".to_string(), Value::String(t));
                    }
                    // Pass through budget if provided
                    if let Some(b) = map.get(&MapKey::String("budget".to_string())) {
                        // Pass as JSON string to preserve structure in minimal MVP
                         let json_structure = rtfs_value_to_json(b)?;
                         body_map.insert("budget".to_string(), json_to_rtfs_value(&json_structure)?);
                    }

                    let body_json = rtfs_value_to_json(&Value::Map(body_map.into_iter().map(|(k,v)| (MapKey::String(k), v)).collect()))?;
                    let body_str = body_json.to_string();

                    // 3. Execute via http-fetch (loopback to gateway)
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
                        .unwrap_or("");
                    
                    if status >= 400 {
                        return Err(RuntimeError::Generic(format!("Create run failed: {} - {}", status, body)));
                    }

                    // Parse response (expected JSON: {run_id: ..., status: ...})
                    let json_val: serde_json::Value = serde_json::from_str(body)
                        .map_err(|e| RuntimeError::Generic(format!("Invalid JSON response: {}", e)))?;
                    
                    let run_id = json_val.get("run_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let run_status = json_val.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");

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
    
    let mut metadata = HashMap::new();
    metadata.insert("name".to_string(), name.to_string());
    metadata.insert("description".to_string(), description.to_string());
    metadata.insert("endpoint".to_string(), endpoint_url.to_string());
    metadata.insert("method".to_string(), method.to_string());
    metadata.insert("security_level".to_string(), "medium".to_string());
    
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
    use crate::utils::value_conversion::json_to_rtfs_value;
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
                             
                             let response_body = serde_json::json!({
                                 "run_id": "run-123",
                                 "status": "scheduled"
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
            Some("mock-secret".to_string())
        ).await.unwrap();

        let inputs = Value::Map(HashMap::from([
            (MapKey::String("goal".to_string()), Value::String("Take over the world".to_string())),
            (MapKey::String("session_id".to_string()), Value::String("session-1".to_string())),
            (MapKey::String("schedule".to_string()), Value::String("in 10s".to_string())),
        ]));

        let result = marketplace.execute_capability("ccos.run.create", &inputs).await.unwrap();
        
        let Value::Map(out) = result else { panic!("Expected map result") };
        assert_eq!(out.get(&MapKey::String("run_id".to_string())).unwrap().as_string().unwrap(), "run-123");
        assert_eq!(out.get(&MapKey::String("status".to_string())).unwrap().as_string().unwrap(), "scheduled");
    }
}
