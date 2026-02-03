//! Integration tests for Skill Onboarding Capabilities
//!
//! Tests the 5 core onboarding capabilities:
//! - ccos.secrets.set
//! - ccos.memory.store
//! - ccos.memory.get
//! - ccos.approval.request_human_action
//! - ccos.approval.complete

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use ccos::approval::storage_memory::InMemoryApprovalStorage;
use ccos::approval::types::{ApprovalCategory, ApprovalStatus};
use ccos::approval::unified_queue::UnifiedApprovalQueue;
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::secrets::SecretStore;
use ccos::skills::onboarding_capabilities::register_onboarding_capabilities;
use ccos::working_memory::{InMemoryJsonlBackend, WorkingMemory};
use rtfs::ast::MapKey;
use rtfs::runtime::values::Value;
use tokio::sync::RwLock;

/// Helper to create Value from JSON-like structure
fn json_to_value(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Nil
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            Value::Vector(arr.into_iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(MapKey::String(k), json_to_value(v));
            }
            Value::Map(map)
        }
    }
}

/// Helper to convert Value back to JSON for assertions
fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Integer(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(*f).unwrap_or(0.into()),
        ),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Vector(v) => serde_json::Value::Array(v.iter().map(value_to_json).collect()),
        Value::Map(m) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in m {
                if let MapKey::String(key) = k {
                    obj.insert(key.clone(), value_to_json(v));
                }
            }
            serde_json::Value::Object(obj)
        }
        _ => serde_json::Value::Null,
    }
}

/// Helper to create test components
async fn create_test_components() -> (
    Arc<CapabilityMarketplace>,
    Arc<StdMutex<SecretStore>>,
    Arc<StdMutex<WorkingMemory>>,
    Arc<UnifiedApprovalQueue<InMemoryApprovalStorage>>,
) {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let secret_store = Arc::new(StdMutex::new(SecretStore::empty()));
    let backend = InMemoryJsonlBackend::new(None, None, None);
    let working_memory = Arc::new(StdMutex::new(WorkingMemory::new(Box::new(backend))));
    let storage = InMemoryApprovalStorage::new();
    let approval_queue = Arc::new(UnifiedApprovalQueue::new(Arc::new(storage)));

    // Register onboarding capabilities
    register_onboarding_capabilities(
        marketplace.clone(),
        secret_store.clone(),
        working_memory.clone(),
        approval_queue.clone(),
    )
    .await
    .expect("Failed to register onboarding capabilities");

    (marketplace, secret_store, working_memory, approval_queue)
}

/// Helper to execute a capability
async fn execute_capability(
    marketplace: &CapabilityMarketplace,
    capability_id: &str,
    args: Value,
) -> Result<Value, String> {
    marketplace
        .execute_capability(capability_id, &args)
        .await
        .map_err(|e| format!("Capability execution failed: {:?}", e))
}

#[tokio::test]
async fn test_secrets_set_creates_approval() {
    let (marketplace, _secret_store, _working_memory, approval_queue) = create_test_components().await;

    // Call ccos.secrets.set
    let args = json_to_value(serde_json::json!({
        "key": "MOLTBOOK_SECRET",
        "value": "super_secret_token_123",
        "scope": "skill",
        "skill_id": "moltbook",
        "description": "API key for Moltbook integration"
    }));

    let result = execute_capability(&marketplace, "ccos.secrets.set", args)
        .await
        .expect("Failed to execute ccos.secrets.set");

    // Verify success response
    let result_json = value_to_json(&result);
    assert!(result_json["success"].as_bool().unwrap());
    assert!(result_json["approval_id"].is_string());
    assert!(result_json["message"].as_str().unwrap().contains("MOLTBOOK_SECRET"));

    // Verify approval was created
    let approval_id = result_json["approval_id"].as_str().unwrap();
    let approval = approval_queue
        .get(approval_id)
        .await
        .expect("Failed to get approval")
        .expect("Approval not found");

    assert!(matches!(approval.status, ApprovalStatus::Pending));
    match &approval.category {
        ApprovalCategory::SecretWrite { key, scope, skill_id, description } => {
            assert_eq!(key, "MOLTBOOK_SECRET");
            assert_eq!(scope, "skill");
            assert_eq!(skill_id.as_ref().unwrap(), "moltbook");
            assert!(description.contains("Moltbook"));
        }
        _ => panic!("Expected SecretWrite approval category"),
    }
}

#[tokio::test]
async fn test_memory_store_and_get() {
    let (marketplace, _secret_store, _working_memory, _approval_queue) = create_test_components().await;

    // Test 1: Store a value
    let store_args = json_to_value(serde_json::json!({
        "key": "agent_id",
        "value": "moltbook_agent_456",
        "skill_id": "moltbook"
    }));

    let store_result = execute_capability(&marketplace, "ccos.memory.store", store_args)
        .await
        .expect("Failed to store value");

    let store_json = value_to_json(&store_result);
    assert!(store_json["success"].as_bool().unwrap());
    assert_eq!(store_json["entry_id"].as_str().unwrap(), "skill:moltbook:agent_id");

    // Test 2: Retrieve the value
    let get_args = json_to_value(serde_json::json!({
        "key": "agent_id",
        "skill_id": "moltbook"
    }));

    let get_result = execute_capability(&marketplace, "ccos.memory.get", get_args)
        .await
        .expect("Failed to get value");

    let get_json = value_to_json(&get_result);
    assert!(get_json["found"].as_bool().unwrap());
    assert!(!get_json["expired"].as_bool().unwrap());
    assert_eq!(get_json["value"].as_str().unwrap(), "moltbook_agent_456");
}

#[tokio::test]
async fn test_memory_get_with_default() {
    let (marketplace, _secret_store, _working_memory, _approval_queue) = create_test_components().await;

    // Try to get non-existent key with default
    let get_args = json_to_value(serde_json::json!({
        "key": "non_existent_key",
        "skill_id": "moltbook",
        "default": "default_value"
    }));

    let get_result = execute_capability(&marketplace, "ccos.memory.get", get_args)
        .await
        .expect("Failed to get value");

    let get_json = value_to_json(&get_result);
    assert!(!get_json["found"].as_bool().unwrap());
    assert!(!get_json["expired"].as_bool().unwrap());
    assert_eq!(get_json["value"].as_str().unwrap(), "default_value");
}

#[tokio::test]
async fn test_memory_ttl_expiration() {
    let (marketplace, _secret_store, _working_memory, _approval_queue) = create_test_components().await;

    // Store a value with very short TTL (1 second)
    let store_args = json_to_value(serde_json::json!({
        "key": "temp_data",
        "value": "temporary",
        "skill_id": "moltbook",
        "ttl": 1
    }));

    execute_capability(&marketplace, "ccos.memory.store", store_args)
        .await
        .expect("Failed to store value");

    // Wait for TTL to expire
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Try to retrieve - should be expired
    let get_args = json_to_value(serde_json::json!({
        "key": "temp_data",
        "skill_id": "moltbook"
    }));

    let get_result = execute_capability(&marketplace, "ccos.memory.get", get_args)
        .await
        .expect("Failed to get value");

    let get_json = value_to_json(&get_result);
    assert!(get_json["found"].as_bool().unwrap()); // Entry exists
    assert!(get_json["expired"].as_bool().unwrap()); // But is expired
    assert!(get_json["value"].is_null()); // Value is None because expired
}

#[tokio::test]
async fn test_request_human_action() {
    let (marketplace, _secret_store, _working_memory, approval_queue) = create_test_components().await;

    // Request human action
    let args = json_to_value(serde_json::json!({
        "action_type": "tweet_verification",
        "title": "Verify Moltbook Agent Ownership",
        "instructions": "Please post the following tweet from your X account:\n\nI'm verifying my AI agent on @moltbook.\n\nThen paste the tweet URL below.",
        "required_response": {
            "type": "object",
            "properties": {
                "tweet_url": { "type": "string" }
            },
            "required": ["tweet_url"]
        },
        "timeout_hours": 48,
        "skill_id": "moltbook",
        "step_id": "tweet-verification"
    }));

    let result = execute_capability(&marketplace, "ccos.approval.request_human_action", args)
        .await
        .expect("Failed to request human action");

    let result_json = value_to_json(&result);
    assert!(result_json["approval_id"].is_string());
    assert_eq!(result_json["status"].as_str().unwrap(), "pending");
    assert!(result_json["expires_at"].is_string());

    // Verify approval was created
    let approval_id = result_json["approval_id"].as_str().unwrap();
    let approval = approval_queue
        .get(approval_id)
        .await
        .expect("Failed to get approval")
        .expect("Approval not found");

    match &approval.category {
        ApprovalCategory::HumanActionRequest { 
            action_type, 
            title, 
            skill_id, 
            step_id,
            timeout_hours,
            ..
        } => {
            assert_eq!(action_type, "tweet_verification");
            assert_eq!(title, "Verify Moltbook Agent Ownership");
            assert_eq!(skill_id, "moltbook");
            assert_eq!(step_id, "tweet-verification");
            assert_eq!(*timeout_hours, 48);
        }
        _ => panic!("Expected HumanActionRequest approval category"),
    }
}

#[tokio::test]
async fn test_complete_human_action() {
    let (marketplace, _secret_store, _working_memory, approval_queue) = create_test_components().await;

    // Step 1: Request human action
    let request_args = json_to_value(serde_json::json!({
        "action_type": "tweet_verification",
        "title": "Verify Moltbook Agent",
        "instructions": "Post tweet and provide URL",
        "required_response": {
            "type": "object",
            "properties": {
                "tweet_url": { "type": "string" }
            },
            "required": ["tweet_url"]
        },
        "skill_id": "moltbook",
        "step_id": "tweet-verification"
    }));

    let request_result = execute_capability(
        &marketplace,
        "ccos.approval.request_human_action",
        request_args,
    )
    .await
    .expect("Failed to request human action");

    let request_json = value_to_json(&request_result);
    let approval_id = request_json["approval_id"].as_str().unwrap();

    // Step 2: Complete with valid response
    let complete_args = json_to_value(serde_json::json!({
        "approval_id": approval_id,
        "response": {
            "tweet_url": "https://x.com/user/status/123456"
        }
    }));

    let complete_result = execute_capability(&marketplace, "ccos.approval.complete", complete_args)
        .await
        .expect("Failed to complete human action");

    let complete_json = value_to_json(&complete_result);
    assert!(complete_json["success"].as_bool().unwrap_or(false), "Expected success to be true");
    // validation_errors might be null or missing when empty, so check more defensively
    let has_no_errors = complete_json["validation_errors"].as_array().map(|arr| arr.is_empty()).unwrap_or(true);
    assert!(has_no_errors, "Expected no validation errors");

    // Step 3: Verify approval is now approved with response
    let approval = approval_queue
        .get(approval_id)
        .await
        .expect("Failed to get approval")
        .expect("Approval not found");

    assert!(matches!(approval.status, ApprovalStatus::Approved { .. }));
    assert!(approval.response.is_some());
    let response = approval.response.as_ref().unwrap();
    assert_eq!(
        response["tweet_url"].as_str().unwrap(),
        "https://x.com/user/status/123456"
    );
}

#[tokio::test]
async fn test_complete_human_action_validation_failure() {
    let (marketplace, _secret_store, _working_memory, _approval_queue) = create_test_components().await;

    // Step 1: Request human action with required field
    let request_args = json_to_value(serde_json::json!({
        "action_type": "email_verification",
        "title": "Verify Email",
        "instructions": "Enter verification code",
        "required_response": {
            "type": "object",
            "properties": {
                "code": { "type": "string" }
            },
            "required": ["code"]
        },
        "skill_id": "test",
        "step_id": "email-verify"
    }));

    let request_result = execute_capability(
        &marketplace,
        "ccos.approval.request_human_action",
        request_args,
    )
    .await
    .expect("Failed to request human action");

    let request_json = value_to_json(&request_result);
    let approval_id = request_json["approval_id"].as_str().unwrap();

    // Step 2: Try to complete with invalid response (missing required field)
    let complete_args = json_to_value(serde_json::json!({
        "approval_id": approval_id,
        "response": {
            "wrong_field": "12345"
        }
    }));

    let complete_result = execute_capability(&marketplace, "ccos.approval.complete", complete_args)
        .await
        .expect("Failed to execute complete");

    let complete_json = value_to_json(&complete_result);
    assert!(!complete_json["success"].as_bool().unwrap());
    assert!(!complete_json["validation_errors"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_secrets_scope_global() {
    let (marketplace, _secret_store, _working_memory, approval_queue) = create_test_components().await;

    // Store secret with global scope
    let args = json_to_value(serde_json::json!({
        "key": "GLOBAL_API_KEY",
        "value": "global_secret",
        "scope": "global",
        "description": "Global API key"
    }));

    let result = execute_capability(&marketplace, "ccos.secrets.set", args)
        .await
        .expect("Failed to execute ccos.secrets.set");

    let result_json = value_to_json(&result);
    assert!(result_json["success"].as_bool().unwrap());

    // Verify approval tracks global scope
    let approval_id = result_json["approval_id"].as_str().unwrap();
    let approval = approval_queue
        .get(approval_id)
        .await
        .expect("Failed to get approval")
        .expect("Approval not found");

    match &approval.category {
        ApprovalCategory::SecretWrite { scope, skill_id, .. } => {
            assert_eq!(scope, "global");
            assert!(skill_id.is_none()); // No skill_id for global scope
        }
        _ => panic!("Expected SecretWrite approval category"),
    }
}

#[tokio::test]
async fn test_approval_listing_by_category() {
    let (marketplace, _secret_store, _working_memory, approval_queue) = create_test_components().await;

    // Create multiple types of approvals
    let secret_args = json_to_value(serde_json::json!({
        "key": "TEST_SECRET",
        "value": "secret",
        "scope": "skill",
        "skill_id": "test"
    }));
    execute_capability(&marketplace, "ccos.secrets.set", secret_args)
        .await
        .expect("Failed to create secret approval");

    let human_action_args = json_to_value(serde_json::json!({
        "action_type": "test",
        "title": "Test",
        "instructions": "Test",
        "skill_id": "test",
        "step_id": "test"
    }));
    execute_capability(&marketplace, "ccos.approval.request_human_action", human_action_args)
        .await
        .expect("Failed to create human action approval");

    // List SecretWrite approvals
    let secret_approvals = approval_queue
        .list_pending_by_category("SecretWrite")
        .await
        .expect("Failed to list approvals");
    assert_eq!(secret_approvals.len(), 1);

    // List HumanActionRequest approvals
    let human_approvals = approval_queue
        .list_pending_by_category("HumanActionRequest")
        .await
        .expect("Failed to list approvals");
    assert_eq!(human_approvals.len(), 1);
}
