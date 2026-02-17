//! Skill Onboarding Capabilities
//!
//! Provides capabilities for multi-step skill onboarding with governance:
//! - ccos.secrets.set: Store secrets with approval
//! - ccos.memory.store: Persist onboarding state
//! - ccos.memory.get: Retrieve onboarding state
//! - ccos.approval.request_human_action: Request human intervention
//! - ccos.approval.complete: Complete human action with response

use crate::approval::queue::{ApprovalAuthority, RiskAssessment, RiskLevel};
use crate::approval::types::{ApprovalCategory, ApprovalRequest, ApprovalStorage};
use crate::approval::unified_queue::UnifiedApprovalQueue;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::secrets::SecretStore;
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};

use crate::working_memory::facade::WorkingMemory;
use crate::working_memory::types::{WorkingMemoryEntry, WorkingMemoryMeta};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};

// =============================================================================
// Input/Output Structs
// =============================================================================

/// Input for ccos.secrets.set
#[derive(Debug, Deserialize)]
struct SecretSetInput {
    key: String,
    value: String,
    #[serde(default = "default_secret_scope")]
    scope: String,
    skill_id: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    session_id: Option<String>,
}

fn default_secret_scope() -> String {
    "skill".to_string()
}

/// Output for ccos.secrets.set
#[derive(Debug, Serialize)]
struct SecretSetOutput {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    approval_id: Option<String>,
    message: String,
}

/// Input for ccos.memory.store
#[derive(Debug, Deserialize)]
struct MemoryStoreInput {
    key: String,
    value: serde_json::Value,
    skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<u64>,
    /// Optional session ID used to tag entries for cross-run retrieval (recurring scheduled runs).
    #[serde(default)]
    session_id: Option<String>,
}

/// Output for ccos.memory.store
#[derive(Debug, Serialize)]
struct MemoryStoreOutput {
    success: bool,
    entry_id: String,
}

/// Input for ccos.memory.get
#[derive(Debug, Deserialize)]
struct MemoryGetInput {
    key: String,
    skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<serde_json::Value>,
}

/// Output for ccos.memory.get
#[derive(Debug, Serialize)]
struct MemoryGetOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
    found: bool,
    expired: bool,
}

/// Input for ccos.approval.request_human_action
#[derive(Debug, Deserialize)]
struct RequestHumanActionInput {
    action_type: String,
    title: String,
    instructions: String,
    #[serde(default = "default_response_schema")]
    required_response: serde_json::Value,
    #[serde(default = "default_timeout")]
    timeout_hours: i64,
    skill_id: String,
    step_id: String,
    #[serde(default)]
    session_id: Option<String>,
}

fn default_timeout() -> i64 {
    24
}

fn default_response_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}

/// Output for ccos.approval.request_human_action
#[derive(Debug, Serialize)]
struct RequestHumanActionOutput {
    approval_id: String,
    status: String,
    expires_at: String,
}

/// Input for ccos.approval.complete
#[derive(Debug, Deserialize)]
struct CompleteHumanActionInput {
    approval_id: String,
    response: serde_json::Value,
}

/// Output for ccos.approval.complete
#[derive(Debug, Serialize)]
struct CompleteHumanActionOutput {
    success: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    validation_errors: Vec<String>,
    message: String,
}

/// Input for ccos.approval.get_status
#[derive(Debug, Deserialize)]
struct GetApprovalStatusInput {
    approval_id: String,
}

/// Output for ccos.approval.get_status
#[derive(Debug, Serialize)]
struct GetApprovalStatusOutput {
    approval_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    approved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rejected_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rejection_reason: Option<String>,
}

/// Input for ccos.skill.get_onboarding_status
#[derive(Debug, Deserialize)]
struct GetOnboardingStatusInput {
    skill_id: String,
}

/// Output for ccos.skill.get_onboarding_status
#[derive(Debug, Serialize)]
struct GetOnboardingStatusOutput {
    skill_id: String,
    has_onboarding: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_step: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_steps: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_steps: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_approval_id: Option<String>,
}

/// Input for ccos.skill.check_onboarding_resume
#[derive(Debug, Deserialize)]
struct CheckOnboardingResumeInput {
    skill_id: String,
}

/// Output for ccos.skill.check_onboarding_resume
#[derive(Debug, Serialize)]
struct CheckOnboardingResumeOutput {
    skill_id: String,
    can_resume: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_step: Option<usize>,
}

/// Input for ccos.skill.onboarding.complete_step
#[derive(Debug, Deserialize)]
struct CompleteOnboardingStepInput {
    skill_id: String,
    step_id: String,
    #[serde(default)]
    data: std::collections::HashMap<String, serde_json::Value>,
}

/// Input for ccos.skill.onboarding.mark_operational
#[derive(Debug, Deserialize)]
struct MarkOperationalInput {
    skill_id: String,
}

// =============================================================================
// Schema Helpers
// =============================================================================

/// Build a MapTypeEntry for a required or optional string field.
fn string_field(name: &str, optional: bool) -> MapTypeEntry {
    MapTypeEntry {
        key: Keyword::new(name),
        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
        optional,
    }
}

/// Build a MapTypeEntry for a required or optional field of any type.
fn any_field(name: &str, optional: bool) -> MapTypeEntry {
    MapTypeEntry {
        key: Keyword::new(name),
        value_type: Box::new(TypeExpr::Any),
        optional,
    }
}

/// Build a MapTypeEntry for a required or optional integer field.
fn int_field(name: &str, optional: bool) -> MapTypeEntry {
    MapTypeEntry {
        key: Keyword::new(name),
        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
        optional,
    }
}

// =============================================================================
// Capability Registration
// =============================================================================

/// Register all onboarding capabilities in the marketplace
pub async fn register_onboarding_capabilities<S: ApprovalStorage + 'static>(
    marketplace: Arc<CapabilityMarketplace>,
    _secret_store: Arc<StdMutex<SecretStore>>,
    working_memory: Arc<StdMutex<WorkingMemory>>,
    approval_queue: Arc<UnifiedApprovalQueue<S>>,
) -> RuntimeResult<()> {
    // ccos.secrets.set
    let approval_queue_set = approval_queue.clone();
    let secrets_set_handler = Arc::new(move |input: &Value| {
        let payload: SecretSetInput = parse_payload("ccos.secrets.set", input)?;
        let queue = approval_queue_set.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_secrets_set(payload, queue).await })
        })
        .join()
        .map_err(|_| RuntimeError::Generic("ccos.secrets.set: thread join error".to_string()))?;

        result
    });

    let secrets_set_schema = TypeExpr::Map {
        entries: vec![
            string_field("key", false),
            string_field("value", false),
            string_field("scope", true),
            string_field("skill_id", true),
            string_field("description", true),
            string_field("session_id", true),
        ],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.secrets.set".to_string(),
            "Onboarding / Secrets Set".to_string(),
            "Store a secret securely with governance approval".to_string(),
            secrets_set_handler,
            Some(secrets_set_schema),
            None,
        )
        .await?;

    // ccos.memory.store
    let working_memory_store = working_memory.clone();
    let memory_store_handler = Arc::new(move |input: &Value| {
        let payload: MemoryStoreInput = parse_payload("ccos.memory.store", input)?;
        let wm = working_memory_store.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_memory_store(payload, wm).await })
        })
        .join()
        .map_err(|_| RuntimeError::Generic("ccos.memory.store: thread join error".to_string()))?;

        result
    });

    let memory_store_schema = TypeExpr::Map {
        entries: vec![
            string_field("key", false),
            any_field("value", false),
            string_field("skill_id", true),
            int_field("ttl", true),
        ],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.memory.store".to_string(),
            "Onboarding / Memory Store".to_string(),
            "Store key-value data in working memory".to_string(),
            memory_store_handler,
            Some(memory_store_schema),
            None,
        )
        .await?;

    // ccos.memory.get
    let working_memory_get = working_memory.clone();
    let memory_get_handler = Arc::new(move |input: &Value| {
        let payload: MemoryGetInput = parse_payload("ccos.memory.get", input)?;
        let wm = working_memory_get.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_memory_get(payload, wm).await })
        })
        .join()
        .map_err(|_| RuntimeError::Generic("ccos.memory.get: thread join error".to_string()))?;

        result
    });

    let memory_get_schema = TypeExpr::Map {
        entries: vec![
            string_field("key", false),
            string_field("skill_id", true),
            any_field("default", true),
        ],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.memory.get".to_string(),
            "Onboarding / Memory Get".to_string(),
            "Retrieve value from working memory".to_string(),
            memory_get_handler,
            Some(memory_get_schema),
            None,
        )
        .await?;

    // ccos.approval.request_human_action
    let approval_queue_request = approval_queue.clone();
    let request_human_action_handler = Arc::new(move |input: &Value| {
        let payload: RequestHumanActionInput =
            parse_payload("ccos.approval.request_human_action", input)?;
        let queue = approval_queue_request.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_request_human_action(payload, queue).await })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic(
                "ccos.approval.request_human_action: thread join error".to_string(),
            )
        })?;

        result
    });

    let request_human_action_schema = TypeExpr::Map {
        entries: vec![
            string_field("action_type", false),
            string_field("title", false),
            string_field("instructions", false),
            any_field("required_response", true),
            int_field("timeout_hours", true),
            string_field("skill_id", false),
            string_field("step_id", false),
            string_field("session_id", true),
        ],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.approval.request_human_action".to_string(),
            "Onboarding / Request Human Action".to_string(),
            "Request human intervention for onboarding steps".to_string(),
            request_human_action_handler,
            Some(request_human_action_schema),
            None,
        )
        .await?;

    // ccos.approval.complete
    let approval_queue_complete = approval_queue.clone();
    let working_memory_complete = working_memory.clone();
    let complete_human_action_handler = Arc::new(move |input: &Value| {
        let payload: CompleteHumanActionInput = parse_payload("ccos.approval.complete", input)?;
        let queue = approval_queue_complete.clone();
        let wm = working_memory_complete.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_complete_human_action(payload, queue, wm).await })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("ccos.approval.complete: thread join error".to_string())
        })?;

        result
    });

    let approval_complete_schema = TypeExpr::Map {
        entries: vec![
            string_field("approval_id", false),
            any_field("response", false),
        ],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.approval.complete".to_string(),
            "Onboarding / Complete Human Action".to_string(),
            "Complete a human action with response data".to_string(),
            complete_human_action_handler,
            Some(approval_complete_schema),
            None,
        )
        .await?;

    // ccos.approval.get_status
    let approval_queue_get_status = approval_queue.clone();
    let get_approval_status_handler = Arc::new(move |input: &Value| {
        let payload: GetApprovalStatusInput = parse_payload("ccos.approval.get_status", input)?;
        let queue = approval_queue_get_status.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_get_approval_status(payload, queue).await })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("ccos.approval.get_status: thread join error".to_string())
        })?;

        result
    });

    let get_approval_status_schema = TypeExpr::Map {
        entries: vec![string_field("approval_id", false)],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.approval.get_status".to_string(),
            "Onboarding / Get Approval Status".to_string(),
            "Get the current status of an approval request".to_string(),
            get_approval_status_handler,
            Some(get_approval_status_schema),
            None,
        )
        .await?;

    // ccos.skill.get_onboarding_status
    let working_memory_status = working_memory.clone();
    let get_onboarding_status_handler = Arc::new(move |input: &Value| {
        let payload: GetOnboardingStatusInput =
            parse_payload("ccos.skill.get_onboarding_status", input)?;
        let wm = working_memory_status.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_get_onboarding_status(payload, wm).await })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("ccos.skill.get_onboarding_status: thread join error".to_string())
        })?;

        result
    });

    let onboarding_status_schema = TypeExpr::Map {
        entries: vec![string_field("skill_id", false)],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.skill.get_onboarding_status".to_string(),
            "Skill / Get Onboarding Status".to_string(),
            "Get the onboarding status for a skill".to_string(),
            get_onboarding_status_handler,
            Some(onboarding_status_schema),
            None,
        )
        .await?;

    // ccos.skill.check_onboarding_resume
    let working_memory_resume = working_memory.clone();
    let approval_queue_resume = approval_queue.clone();
    let check_onboarding_resume_handler = Arc::new(move |input: &Value| {
        let payload: CheckOnboardingResumeInput =
            parse_payload("ccos.skill.check_onboarding_resume", input)?;
        let wm = working_memory_resume.clone();
        let queue = approval_queue_resume.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async { handle_check_onboarding_resume(payload, wm, queue).await })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic(
                "ccos.skill.check_onboarding_resume: thread join error".to_string(),
            )
        })?;

        result
    });

    let onboarding_resume_schema = TypeExpr::Map {
        entries: vec![string_field("skill_id", false)],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.skill.check_onboarding_resume".to_string(),
            "Skill / Check Onboarding Resume".to_string(),
            "Check if onboarding can be resumed after human action".to_string(),
            check_onboarding_resume_handler,
            Some(onboarding_resume_schema),
            None,
        )
        .await?;

    // ccos.skill.onboarding.complete_step
    let working_memory_complete_step = working_memory.clone();
    let complete_step_handler = Arc::new(move |input: &Value| {
        let payload: CompleteOnboardingStepInput =
            parse_payload("ccos.skill.onboarding.complete_step", input)?;
        let wm = working_memory_complete_step.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let osm = crate::skills::onboarding_state_machine::OnboardingStateMachine::new(wm);
                let mut state = osm.get_state(&payload.skill_id).ok_or_else(|| {
                    RuntimeError::Generic(format!("No onboarding state for {}", payload.skill_id))
                })?;

                osm.complete_step(&payload.skill_id, payload.step_id, &mut state, payload.data)
                    .map_err(|e| RuntimeError::Generic(format!("{}", e)))?;

                produce_value(
                    "ccos.skill.onboarding.complete_step",
                    serde_json::json!({"success": true}),
                )
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic(
                "ccos.skill.onboarding.complete_step: thread join error".to_string(),
            )
        })?;

        result
    });

    let complete_step_schema = TypeExpr::Map {
        entries: vec![
            string_field("skill_id", false),
            string_field("step_id", false),
            any_field("data", true),
        ],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.skill.onboarding.complete_step".to_string(),
            "Skill / Onboarding Complete Step".to_string(),
            "Mark an onboarding step as complete".to_string(),
            complete_step_handler,
            Some(complete_step_schema),
            None,
        )
        .await?;

    // ccos.skill.onboarding.mark_operational
    let working_memory_mark_op = working_memory.clone();
    let mark_operational_handler = Arc::new(move |input: &Value| {
        let payload: MarkOperationalInput =
            parse_payload("ccos.skill.onboarding.mark_operational", input)?;
        let wm = working_memory_mark_op.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let osm = crate::skills::onboarding_state_machine::OnboardingStateMachine::new(
                    wm.clone(),
                );
                let mut state = osm.get_state(&payload.skill_id).ok_or_else(|| {
                    RuntimeError::Generic(format!("No onboarding state for {}", payload.skill_id))
                })?;

                state.status = crate::skills::types::OnboardingState::Operational;

                let key = format!("skill:{}:onboarding_state", payload.skill_id);
                let content = serde_json::to_string(&state)
                    .map_err(|e| RuntimeError::Generic(e.to_string()))?;

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let entry = crate::working_memory::types::WorkingMemoryEntry::new_with_estimate(
                    key.clone(),
                    format!("Onboarding state for {}", payload.skill_id),
                    content,
                    vec![
                        "onboarding".to_string(),
                        format!("skill:{}", payload.skill_id),
                    ],
                    now,
                    crate::working_memory::types::WorkingMemoryMeta::default(),
                );

                let mut wm_guard = wm
                    .lock()
                    .map_err(|_| RuntimeError::Generic("WM lock failed".to_string()))?;
                wm_guard
                    .append(entry)
                    .map_err(|e| RuntimeError::Generic(format!("{}", e)))?;

                produce_value(
                    "ccos.skill.onboarding.mark_operational",
                    serde_json::json!({"success": true}),
                )
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic(
                "ccos.skill.onboarding.mark_operational: thread join error".to_string(),
            )
        })?;

        result
    });

    let mark_operational_schema = TypeExpr::Map {
        entries: vec![string_field("skill_id", false)],
        wildcard: None,
    };

    marketplace
        .register_local_capability_with_schema(
            "ccos.skill.onboarding.mark_operational".to_string(),
            "Skill / Onboarding Mark Operational".to_string(),
            "Mark a skill as fully operational".to_string(),
            mark_operational_handler,
            Some(mark_operational_schema),
            None,
        )
        .await?;

    Ok(())
}

// =============================================================================
// Capability Handlers
// =============================================================================

async fn handle_secrets_set<S: ApprovalStorage>(
    payload: SecretSetInput,
    approval_queue: Arc<UnifiedApprovalQueue<S>>,
) -> RuntimeResult<Value> {
    // Create SecretWrite approval
    let category = ApprovalCategory::SecretWrite {
        key: payload.key.clone(),
        scope: payload.scope.clone(),
        skill_id: payload.skill_id.clone(),
        description: if payload.description.is_empty() {
            format!("Secret '{}' for skill onboarding", payload.key)
        } else {
            payload.description.clone()
        },
        value: Some(payload.value.clone()),
    };

    let risk = RiskAssessment {
        level: RiskLevel::High,
        reasons: vec!["Storing sensitive credential".to_string()],
    };

    let mut request = ApprovalRequest::new(category, risk, 24, None);

    // Add metadata for correlation
    if let Some(session_id) = payload.session_id {
        request
            .metadata
            .insert("session_id".to_string(), session_id);
    }
    if let Some(skill_id) = payload.skill_id {
        request.metadata.insert("skill_id".to_string(), skill_id);
    }

    let approval_id = request.id.clone();

    approval_queue
        .add(request)
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to create approval: {}", e)))?;

    let output = SecretSetOutput {
        success: true,
        approval_id: Some(approval_id),
        message: format!(
            "Secret '{}' staged with scope '{}'. Approval required before persistence.",
            payload.key, payload.scope
        ),
    };

    produce_value("ccos.secrets.set", output)
}

/// Handle ccos.memory.store
async fn handle_memory_store(
    payload: MemoryStoreInput,
    working_memory: Arc<StdMutex<WorkingMemory>>,
) -> RuntimeResult<Value> {
    let entry_id = if let Some(ref skill_id) = payload.skill_id {
        format!("skill:{}:{}", skill_id, payload.key)
    } else {
        format!("global:{}", payload.key)
    };

    let mut tags: HashSet<String> = HashSet::new();
    tags.insert("onboarding".to_string());
    if let Some(ref skill_id) = payload.skill_id {
        tags.insert(format!("skill:{}", skill_id));
    }
    // Tag with session ID when provided so cross-run WM queries can filter by session.
    if let Some(ref sid) = payload.session_id {
        tags.insert(format!("session:{}", sid));
    }

    let content = serde_json::to_string(&payload.value)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize value: {}", e)))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut meta = WorkingMemoryMeta::default();
    if let Some(ttl) = payload.ttl {
        meta.extra.insert("ttl".to_string(), ttl.to_string());
        meta.extra
            .insert("expires_at".to_string(), (now + ttl).to_string());
    }

    let entry = WorkingMemoryEntry::new_with_estimate(
        entry_id.clone(),
        payload.key.clone(),
        content,
        tags,
        now,
        meta,
    );

    let mut wm = working_memory
        .lock()
        .map_err(|_| RuntimeError::Generic("Failed to lock working memory".to_string()))?;
    wm.append(entry)
        .map_err(|e| RuntimeError::Generic(format!("Failed to store in working memory: {}", e)))?;

    let output = MemoryStoreOutput {
        success: true,
        entry_id,
    };

    produce_value("ccos.memory.store", output)
}

/// Handle ccos.memory.get
async fn handle_memory_get(
    payload: MemoryGetInput,
    working_memory: Arc<StdMutex<WorkingMemory>>,
) -> RuntimeResult<Value> {
    let entry_id = if let Some(ref skill_id) = payload.skill_id {
        format!("skill:{}:{}", skill_id, payload.key)
    } else {
        format!("global:{}", payload.key)
    };

    let wm = working_memory
        .lock()
        .map_err(|_| RuntimeError::Generic("Failed to lock working memory".to_string()))?;

    match wm.get(&entry_id) {
        Ok(Some(entry)) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Check TTL
            let expired = if let Some(expires_at_str) = entry.meta.extra.get("expires_at") {
                if let Ok(expires_at) = expires_at_str.parse::<u64>() {
                    now > expires_at
                } else {
                    false
                }
            } else {
                false
            };

            if expired {
                let output = MemoryGetOutput {
                    value: None,
                    found: true,
                    expired: true,
                };
                return produce_value("ccos.memory.get", output);
            }

            let value: serde_json::Value = serde_json::from_str(&entry.content).map_err(|e| {
                RuntimeError::Generic(format!("Failed to deserialize value: {}", e))
            })?;

            let output = MemoryGetOutput {
                value: Some(value),
                found: true,
                expired: false,
            };
            produce_value("ccos.memory.get", output)
        }
        Ok(None) => {
            let output = MemoryGetOutput {
                value: payload.default,
                found: false,
                expired: false,
            };
            produce_value("ccos.memory.get", output)
        }
        Err(e) => Err(RuntimeError::Generic(format!(
            "Failed to retrieve from working memory: {}",
            e
        ))),
    }
}

/// Handle ccos.approval.request_human_action
async fn handle_request_human_action<S: ApprovalStorage>(
    payload: RequestHumanActionInput,
    approval_queue: Arc<UnifiedApprovalQueue<S>>,
) -> RuntimeResult<Value> {
    let category = ApprovalCategory::HumanActionRequest {
        action_type: payload.action_type.clone(),
        title: payload.title.clone(),
        instructions: payload.instructions.clone(),
        required_response_schema: payload.required_response.clone(),
        timeout_hours: payload.timeout_hours,
        skill_id: payload.skill_id.clone(),
        step_id: payload.step_id.clone(),
    };

    let risk = RiskAssessment {
        level: RiskLevel::Medium,
        reasons: vec!["Requires human intervention".to_string()],
    };

    // Build metadata with session_id for callback notifications
    let mut request = ApprovalRequest::new(category, risk, payload.timeout_hours, None);
    if let Some(session_id) = payload.session_id {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("session_id".to_string(), session_id);
        request = request.with_metadata(metadata);
    }
    let approval_id = request.id.clone();
    let expires_at = request.expires_at.clone();

    approval_queue
        .add(request)
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to create approval: {}", e)))?;

    let output = RequestHumanActionOutput {
        approval_id,
        status: "pending".to_string(),
        expires_at: expires_at.to_rfc3339(),
    };

    produce_value("ccos.approval.request_human_action", output)
}

/// Handle ccos.approval.complete
async fn handle_complete_human_action<S: ApprovalStorage>(
    payload: CompleteHumanActionInput,
    approval_queue: Arc<UnifiedApprovalQueue<S>>,
    working_memory: Arc<StdMutex<WorkingMemory>>,
) -> RuntimeResult<Value> {
    let mut approval = approval_queue
        .get(&payload.approval_id)
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to get approval: {}", e)))?
        .ok_or_else(|| {
            RuntimeError::Generic(format!("Approval not found: {}", payload.approval_id))
        })?;

    // Validate it's a HumanActionRequest
    let required_schema = match &approval.category {
        ApprovalCategory::HumanActionRequest {
            required_response_schema,
            ..
        } => required_response_schema.clone(),
        _ => {
            return Err(RuntimeError::Generic(
                "Approval is not a human action request".to_string(),
            ));
        }
    };

    // Validate response against schema
    let validation_errors = validate_response(&payload.response, &required_schema);

    if !validation_errors.is_empty() {
        let output = CompleteHumanActionOutput {
            success: false,
            validation_errors,
            message: "Response validation failed".to_string(),
        };
        return produce_value("ccos.approval.complete", output);
    }

    // Store response in approval
    approval.response = Some(payload.response.clone());

    // Approve the request
    approval.approve(
        ApprovalAuthority::Auto,
        Some("Human action completed successfully".to_string()),
    );

    approval_queue
        .update(&approval)
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to update approval: {}", e)))?;

    // Also store in WorkingMemory for easy access
    let entry_id = format!("approval:{}:response", payload.approval_id);
    let content = serde_json::to_string(&payload.response)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize response: {}", e)))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let entry = WorkingMemoryEntry::new_with_estimate(
        entry_id,
        "Human Action Response".to_string(),
        content,
        vec!["approval-response".to_string()],
        now,
        WorkingMemoryMeta::default(),
    );

    let mut wm = working_memory
        .lock()
        .map_err(|_| RuntimeError::Generic("Failed to lock working memory".to_string()))?;
    wm.append(entry)
        .map_err(|e| RuntimeError::Generic(format!("Failed to store response: {}", e)))?;

    let output = CompleteHumanActionOutput {
        success: true,
        validation_errors: vec![],
        message: "Human action completed successfully".to_string(),
    };

    produce_value("ccos.approval.complete", output)
}

/// Handle ccos.approval.get_status
async fn handle_get_approval_status<S: ApprovalStorage>(
    payload: GetApprovalStatusInput,
    approval_queue: Arc<UnifiedApprovalQueue<S>>,
) -> RuntimeResult<Value> {
    let approval = approval_queue
        .get(&payload.approval_id)
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to get approval: {}", e)))?
        .ok_or_else(|| {
            RuntimeError::Generic(format!("Approval not found: {}", payload.approval_id))
        })?;

    let (status, approved_at, rejected_at, rejection_reason) = match &approval.status {
        crate::approval::types::ApprovalStatus::Pending => {
            ("pending".to_string(), None, None, None)
        }
        crate::approval::types::ApprovalStatus::Approved { at, .. } => {
            ("approved".to_string(), Some(at.to_rfc3339()), None, None)
        }
        crate::approval::types::ApprovalStatus::Rejected { at, reason, .. } => {
            ("rejected".to_string(), None, Some(at.to_rfc3339()), Some(reason.clone()))
        }
        crate::approval::types::ApprovalStatus::Expired { at: _ } => {
            ("expired".to_string(), None, None, None)
        }
        crate::approval::types::ApprovalStatus::Superseded { .. } => {
            ("superseded".to_string(), None, None, None)
        }
    };

    let output = GetApprovalStatusOutput {
        approval_id: payload.approval_id.clone(),
        status,
        response: approval.response.clone(),
        approved_at,
        rejected_at,
        rejection_reason,
    };

    produce_value("ccos.approval.get_status", output)
}

// =============================================================================
// Helper Functions
// =============================================================================

fn parse_payload<T: serde::de::DeserializeOwned>(
    capability: &str,
    value: &Value,
) -> RuntimeResult<T> {
    let serialized = rtfs_value_to_json(value)?;
    serde_json::from_value(serialized).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: input payload does not match schema: {}",
            capability, err
        ))
    })
}

fn produce_value<T: Serialize>(capability: &str, output: T) -> RuntimeResult<Value> {
    let json_value = serde_json::to_value(output).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: failed to serialize output: {}",
            capability, err
        ))
    })?;

    json_to_rtfs_value(&json_value)
}

/// Validate response against JSON schema
fn validate_response(response: &serde_json::Value, schema: &serde_json::Value) -> Vec<String> {
    let mut errors = Vec::new();

    // Basic schema validation (type checking)
    if let Some(schema_type) = schema.get("type").and_then(|t| t.as_str()) {
        match schema_type {
            "object" => {
                if !response.is_object() {
                    errors.push(format!("Expected object, got {}", json_type_name(response)));
                } else if let Some(properties) =
                    schema.get("properties").and_then(|p| p.as_object())
                {
                    // Check required properties
                    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
                        for req in required {
                            if let Some(req_str) = req.as_str() {
                                if !response.get(req_str).is_some() {
                                    errors.push(format!("Missing required field: {}", req_str));
                                }
                            }
                        }
                    }
                    // Validate property types
                    for (prop_name, prop_schema) in properties {
                        if let Some(prop_value) = response.get(prop_name) {
                            if let Some(prop_type) =
                                prop_schema.get("type").and_then(|t| t.as_str())
                            {
                                if !validate_type(prop_value, prop_type) {
                                    errors.push(format!(
                                        "Field '{}' expected type '{}', got '{}'",
                                        prop_name,
                                        prop_type,
                                        json_type_name(prop_value)
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            "string" => {
                if !response.is_string() {
                    errors.push(format!("Expected string, got {}", json_type_name(response)));
                }
            }
            "number" => {
                if !response.is_number() {
                    errors.push(format!("Expected number, got {}", json_type_name(response)));
                }
            }
            "array" => {
                if !response.is_array() {
                    errors.push(format!("Expected array, got {}", json_type_name(response)));
                }
            }
            "boolean" => {
                if !response.is_boolean() {
                    errors.push(format!(
                        "Expected boolean, got {}",
                        json_type_name(response)
                    ));
                }
            }
            _ => {}
        }
    }

    errors
}

fn validate_type(value: &serde_json::Value, expected_type: &str) -> bool {
    match expected_type {
        "string" => value.is_string(),
        "number" | "integer" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true, // Unknown types pass
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Handle ccos.skill.get_onboarding_status
async fn handle_get_onboarding_status(
    payload: GetOnboardingStatusInput,
    working_memory: Arc<StdMutex<WorkingMemory>>,
) -> RuntimeResult<Value> {
    use crate::skills::onboarding_state_machine::OnboardingStateMachine;

    let state_machine = OnboardingStateMachine::new(working_memory);
    let summary = state_machine.get_status_summary(&payload.skill_id);

    let output = match summary {
        Some(summary) => GetOnboardingStatusOutput {
            skill_id: payload.skill_id.clone(),
            has_onboarding: true,
            status: Some(format!("{:?}", summary.status)),
            current_step: Some(summary.current_step),
            total_steps: Some(summary.total_steps),
            completed_steps: Some(summary.completed_steps),
            is_complete: Some(summary.is_complete),
            pending_approval_id: summary.pending_approval,
        },
        None => GetOnboardingStatusOutput {
            skill_id: payload.skill_id.clone(),
            has_onboarding: false,
            status: None,
            current_step: None,
            total_steps: None,
            completed_steps: None,
            is_complete: None,
            pending_approval_id: None,
        },
    };

    produce_value("ccos.skill.get_onboarding_status", output)
}

/// Handle ccos.skill.check_onboarding_resume
async fn handle_check_onboarding_resume<S: ApprovalStorage>(
    payload: CheckOnboardingResumeInput,
    working_memory: Arc<StdMutex<WorkingMemory>>,
    approval_queue: Arc<UnifiedApprovalQueue<S>>,
) -> RuntimeResult<Value> {
    use crate::skills::onboarding_state_machine::OnboardingStateMachine;

    let state_machine = OnboardingStateMachine::new(working_memory);

    match state_machine
        .check_and_resume(&payload.skill_id, &approval_queue)
        .await
    {
        Ok(Some(state)) => {
            let output = CheckOnboardingResumeOutput {
                skill_id: payload.skill_id.clone(),
                can_resume: state.status
                    != crate::skills::types::OnboardingState::PendingHumanAction,
                status: Some(format!("{:?}", state.status)),
                message: Some("Onboarding state updated".to_string()),
                next_step: Some(state.current_step),
            };
            produce_value("ccos.skill.check_onboarding_resume", output)
        }
        Ok(None) => {
            let output = CheckOnboardingResumeOutput {
                skill_id: payload.skill_id.clone(),
                can_resume: false,
                status: None,
                message: Some("No onboarding state found".to_string()),
                next_step: None,
            };
            produce_value("ccos.skill.check_onboarding_resume", output)
        }
        Err(e) => {
            let output = CheckOnboardingResumeOutput {
                skill_id: payload.skill_id.clone(),
                can_resume: false,
                status: None,
                message: Some(format!("Error: {}", e)),
                next_step: None,
            };
            produce_value("ccos.skill.check_onboarding_resume", output)
        }
    }
}
