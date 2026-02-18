use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration as StdDuration;

use chrono::Duration;
use tempfile::tempdir;
use tokio::sync::RwLock;

use ccos::approval::{storage_file::FileApprovalStorage, ApprovalAuthority, UnifiedApprovalQueue};
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::chat::{
    approve_request, attach_label, extract_label, filter_mcp_tool_result, record_chat_audit_event,
    register_chat_capabilities, request_chat_policy_exception,
    request_chat_public_declassification, strip_ccos_meta, ChatDataLabel, InMemoryQuarantineStore,
    QuarantineStore,
};
use ccos::types::ActionType;

use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::values::Value;

#[test]
fn test_label_join_with_field_labels() {
    let mut data = HashMap::new();
    data.insert(
        MapKey::String("a".to_string()),
        Value::String("ok".to_string()),
    );
    data.insert(
        MapKey::Keyword(Keyword("a".to_string())),
        Value::String("ok".to_string()),
    );
    data.insert(
        MapKey::String("b".to_string()),
        Value::String("secret".to_string()),
    );
    data.insert(
        MapKey::Keyword(Keyword("b".to_string())),
        Value::String("secret".to_string()),
    );

    let mut field_labels = HashMap::new();
    field_labels.insert("a".to_string(), ChatDataLabel::Public);
    field_labels.insert("b".to_string(), ChatDataLabel::PiiChatMessage);

    let labeled = attach_label(
        Value::Map(data),
        ChatDataLabel::PiiRedacted,
        Some(field_labels),
    );
    assert_eq!(extract_label(&labeled), ChatDataLabel::PiiChatMessage);

    let stripped = strip_ccos_meta(&labeled);
    if let Value::Map(map) = stripped {
        for key in map.keys() {
            let key_str = match key {
                MapKey::String(s) => s.clone(),
                MapKey::Keyword(k) => k.0.clone(),
                MapKey::Integer(i) => i.to_string(),
            };
            assert_ne!(key_str, ccos::chat::CCOS_META_KEY);
        }
    } else {
        panic!("expected map after strip");
    }
}

#[test]
fn test_quarantine_store_ttl_expires() {
    let store = InMemoryQuarantineStore::new();
    let id = store
        .put_bytes(vec![1, 2, 3], Duration::milliseconds(1))
        .expect("put bytes");
    sleep(StdDuration::from_millis(5));
    let result = store.get_bytes(&id);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_prepare_outbound_and_redacted_approval() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
    let quarantine = Arc::new(InMemoryQuarantineStore::new()) as Arc<dyn QuarantineStore>;

    let dir = tempdir().expect("tempdir");
    let storage = FileApprovalStorage::new(dir.path().join("approvals")).expect("storage");
    let queue = UnifiedApprovalQueue::new(Arc::new(storage));

    let resource_store = ccos::chat::new_shared_resource_store();
    let marketplace = Arc::new(marketplace);
    register_chat_capabilities(
        marketplace.clone(),
        quarantine,
        chain,
        Some(queue.clone()),
        resource_store,
        None,
        None,
        None,
        None,
        ccos::config::types::SandboxConfig::default(),
        ccos::config::types::CodingAgentsConfig::default(),
    )
    .await
    .expect("register");

    let session_id = "s1";
    let run_id = "r1";
    let step_id = "step-1";

    let public_value = attach_label(Value::String("ok".to_string()), ChatDataLabel::Public, None);
    let mut inputs = HashMap::new();
    inputs.insert(MapKey::String("content".to_string()), public_value);
    inputs.insert(
        MapKey::String("session_id".to_string()),
        Value::String(session_id.to_string()),
    );
    inputs.insert(
        MapKey::String("run_id".to_string()),
        Value::String(run_id.to_string()),
    );
    inputs.insert(
        MapKey::String("step_id".to_string()),
        Value::String(step_id.to_string()),
    );
    inputs.insert(
        MapKey::String("policy_pack_version".to_string()),
        Value::String("chat-mode-v0".to_string()),
    );

    let result = marketplace
        .execute_capability("ccos.chat.egress.prepare_outbound", &Value::Map(inputs))
        .await;
    assert!(result.is_ok());

    let redacted_value = attach_label(
        Value::String("redacted".to_string()),
        ChatDataLabel::PiiRedacted,
        None,
    );
    let mut inputs2 = HashMap::new();
    inputs2.insert(MapKey::String("content".to_string()), redacted_value);
    inputs2.insert(
        MapKey::String("session_id".to_string()),
        Value::String(session_id.to_string()),
    );
    inputs2.insert(
        MapKey::String("run_id".to_string()),
        Value::String(run_id.to_string()),
    );
    inputs2.insert(
        MapKey::String("step_id".to_string()),
        Value::String(step_id.to_string()),
    );
    inputs2.insert(
        MapKey::String("policy_pack_version".to_string()),
        Value::String("chat-mode-v0".to_string()),
    );

    let denied = marketplace
        .execute_capability(
            "ccos.chat.egress.prepare_outbound",
            &Value::Map(inputs2.clone()),
        )
        .await;
    assert!(denied.is_err());

    let approval_id = request_chat_policy_exception(
        &queue,
        "egress.pii_redacted",
        session_id,
        run_id,
        "test".to_string(),
    )
    .await
    .expect("approval");
    approve_request(
        &queue,
        &approval_id,
        ApprovalAuthority::User("tester".to_string()),
        None,
    )
    .await
    .expect("approve");

    let allowed = marketplace
        .execute_capability("ccos.chat.egress.prepare_outbound", &Value::Map(inputs2))
        .await;
    assert!(allowed.is_ok());
}

#[tokio::test]
async fn test_filter_mcp_tool_result_blocks_pii() {
    let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
    let result = attach_label(
        Value::String("secret".to_string()),
        ChatDataLabel::PiiChatMessage,
        None,
    );

    let filtered = filter_mcp_tool_result(
        &chain,
        None,
        "plan",
        "intent",
        "session",
        "run",
        "step",
        "chat-mode-v0",
        &result,
    )
    .await;

    assert!(filtered.is_err());
}

#[tokio::test]
async fn test_verify_redaction_requires_approval() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
    let quarantine = Arc::new(InMemoryQuarantineStore::new()) as Arc<dyn QuarantineStore>;

    let dir = tempdir().expect("tempdir");
    let storage = FileApprovalStorage::new(dir.path().join("approvals")).expect("storage");
    let queue = UnifiedApprovalQueue::new(Arc::new(storage));

    let resource_store = ccos::chat::new_shared_resource_store();
    let marketplace = Arc::new(marketplace);
    register_chat_capabilities(
        marketplace.clone(),
        quarantine,
        chain,
        Some(queue.clone()),
        resource_store,
        None,
        None,
        None,
        None,
        ccos::config::types::SandboxConfig::default(),
        ccos::config::types::CodingAgentsConfig::default(),
    )
    .await
    .expect("register");

    let mut inputs = HashMap::new();
    inputs.insert(
        MapKey::String("text".to_string()),
        Value::String("ok".to_string()),
    );
    inputs.insert(
        MapKey::String("session_id".to_string()),
        Value::String("s2".to_string()),
    );
    inputs.insert(
        MapKey::String("run_id".to_string()),
        Value::String("r2".to_string()),
    );
    inputs.insert(
        MapKey::String("step_id".to_string()),
        Value::String("step-2".to_string()),
    );

    let denied = marketplace
        .execute_capability(
            "ccos.chat.transform.verify_redaction",
            &Value::Map(inputs.clone()),
        )
        .await;
    assert!(denied.is_err());

    let approval_id = request_chat_public_declassification(
        &queue,
        "s2",
        "r2",
        "ccos.chat.transform.redact_message",
        "ccos.chat.transform.verify_redaction",
        "max_len=280",
        "test".to_string(),
    )
    .await
    .expect("approval");

    approve_request(
        &queue,
        &approval_id,
        ApprovalAuthority::User("tester".to_string()),
        None,
    )
    .await
    .expect("approve");

    let allowed = marketplace
        .execute_capability("ccos.chat.transform.verify_redaction", &Value::Map(inputs))
        .await
        .expect("verify");

    assert_eq!(extract_label(&allowed), ChatDataLabel::Public);
}

#[test]
fn test_record_chat_audit_event_fields() {
    let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
    let mut meta = HashMap::new();
    meta.insert(
        "policy_pack_version".to_string(),
        Value::String("chat-mode-v0".to_string()),
    );
    meta.insert(
        "rule_id".to_string(),
        Value::String("chat.message.ingest".to_string()),
    );

    record_chat_audit_event(
        &chain,
        "plan",
        "intent",
        "session",
        "run",
        "step",
        "message.ingest",
        meta,
        ActionType::InternalStep,
    )
    .expect("record");

    let guard = chain.lock().expect("lock");
    let actions = guard.get_all_actions();
    assert!(!actions.is_empty());
    let last = actions.last().expect("last");
    assert_eq!(
        last.metadata.get("event_type"),
        Some(&Value::String("message.ingest".to_string()))
    );
    assert_eq!(
        last.metadata.get("session_id"),
        Some(&Value::String("session".to_string()))
    );
    assert_eq!(
        last.metadata.get("run_id"),
        Some(&Value::String("run".to_string()))
    );
    assert_eq!(
        last.metadata.get("step_id"),
        Some(&Value::String("step".to_string()))
    );
    assert_eq!(
        last.metadata.get("policy_pack_version"),
        Some(&Value::String("chat-mode-v0".to_string()))
    );
    assert_eq!(
        last.metadata.get("rule_id"),
        Some(&Value::String("chat.message.ingest".to_string()))
    );
}
