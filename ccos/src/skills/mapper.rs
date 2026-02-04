//! Skill Mapper
//!
//! Maps skills to capabilities and executes skill intents.

use super::primitives::PrimitiveMapper;
use crate::approval::storage_file::FileApprovalStorage;
use crate::approval::{
    ApprovalCategory, ApprovalFilter, ApprovalRequest, RiskAssessment, RiskLevel,
    UnifiedApprovalQueue,
};
use crate::capability_marketplace::types::{
    CapabilityManifest, EffectType, NativeCapability, ProviderType, SandboxedCapability,
};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::secrets::SecretStore;
use crate::skills::types::Skill;
use crate::utils::fs::get_workspace_root;
use crate::utils::log_redaction::redact_json_for_logs;
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use futures::future::BoxFuture;
use rtfs::ast::MapKey;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::type_validator::{TypeCheckingConfig, TypeValidator, VerificationContext};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// Error type for skill operations
#[derive(Debug)]
pub enum SkillError {
    /// Skill not found
    NotFound(String),
    /// Capability not available for skill
    CapabilityNotFound(String),
    /// Capability not approved
    NotApproved(String),
    /// Execution error
    Execution(String),
    /// Validation error
    Validation(String),
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillError::NotFound(id) => write!(f, "Skill not found: {}", id),
            SkillError::CapabilityNotFound(id) => write!(f, "Capability not found: {}", id),
            SkillError::NotApproved(id) => write!(f, "Skill not approved: {}", id),
            SkillError::Execution(msg) => write!(f, "Execution error: {}", msg),
            SkillError::Validation(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for SkillError {}

/// Intent representing what the user wants to do
#[derive(Debug, Clone)]
pub struct Intent {
    /// Natural language description of the intent
    pub description: String,
    /// Extracted parameters (if any)
    pub params: HashMap<String, Value>,
    /// Context from conversation
    pub context: HashMap<String, String>,
}

impl Intent {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            params: HashMap::new(),
            context: HashMap::new(),
        }
    }

    pub fn with_param(mut self, key: impl Into<String>, value: Value) -> Self {
        self.params.insert(key.into(), value);
        self
    }
}

/// Maps skills to capabilities and executes skill intents
pub struct SkillMapper {
    /// Registered skills by ID
    skills: HashMap<String, Skill>,
    /// Capability marketplace for resolving capabilities
    marketplace: Arc<CapabilityMarketplace>,
    /// Primitive mapper for mapping commands to capabilities
    _primitive_mapper: crate::skills::PrimitiveMapper,
}

impl SkillMapper {
    /// Create a new skill mapper
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self {
            skills: HashMap::new(),
            marketplace,
            _primitive_mapper: crate::skills::PrimitiveMapper::new(),
        }
    }

    /// Register a skill
    pub fn register_skill(&mut self, skill: Skill) {
        self.skills.insert(skill.id.clone(), skill);
    }

    /// Register multiple skills
    pub fn register_skills(&mut self, skills: Vec<Skill>) {
        for skill in skills {
            self.register_skill(skill);
        }
    }

    /// Get a skill by ID
    pub fn get_skill(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id)
    }

    /// List all registered skills
    pub fn list_skills(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// List visible skills (for UI)
    pub fn list_visible_skills(&self) -> Vec<&Skill> {
        self.skills.values().filter(|s| s.display.visible).collect()
    }

    /// List skills by category
    pub fn list_skills_by_category(&self, category: &str) -> Vec<&Skill> {
        self.skills
            .values()
            .filter(|s| s.display.category == category)
            .collect()
    }

    /// Resolve capabilities for a skill
    /// Returns the capability manifests required by the skill
    pub async fn resolve_capabilities(
        &self,
        skill: &Skill,
    ) -> Result<Vec<CapabilityManifest>, SkillError> {
        let mut manifests = Vec::new();

        for cap_id in &skill.capabilities {
            if self.marketplace.has_capability(cap_id).await {
                if let Some(manifest) = self.marketplace.get_capability(cap_id).await {
                    manifests.push(manifest);
                } else {
                    return Err(SkillError::CapabilityNotFound(cap_id.clone()));
                }
            } else {
                return Err(SkillError::CapabilityNotFound(cap_id.clone()));
            }
        }

        Ok(manifests)
    }

    /// Check if a skill's capabilities are all available
    pub async fn is_skill_available(&self, skill_id: &str) -> bool {
        if let Some(skill) = self.skills.get(skill_id) {
            for cap_id in &skill.capabilities {
                if !self.marketplace.has_capability(cap_id).await {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    /// Execute a skill with the given intent
    /// This is a simplified version - full implementation would use LLM for intent interpretation
    pub async fn execute_skill_intent(
        &self,
        skill_id: &str,
        intent: &Intent,
    ) -> Result<Value, SkillError> {
        let skill = self
            .skills
            .get(skill_id)
            .ok_or_else(|| SkillError::NotFound(skill_id.to_string()))?;

        // Resolve capabilities to ensure they're available
        let _capabilities = self.resolve_capabilities(skill).await?;

        // For now, return a simple acknowledgment
        // Full implementation would:
        // 1. Use LLM to interpret intent with skill instructions
        // 2. Select appropriate capability
        // 3. Route through GovernanceKernel for execution
        // 4. Return result

        // This is a stub - real implementation needs LLM integration
        let result = rtfs::ast::MapKey::String("result".to_string());
        let mut map = std::collections::HashMap::new();
        map.insert(
            result,
            Value::String(format!(
                "Skill '{}' would process intent: {}",
                skill.name, intent.description
            )),
        );
        map.insert(
            rtfs::ast::MapKey::String("skill_id".to_string()),
            Value::String(skill_id.to_string()),
        );
        map.insert(
            rtfs::ast::MapKey::String("capabilities".to_string()),
            Value::List(
                skill
                    .capabilities
                    .iter()
                    .map(|c| Value::String(c.clone()))
                    .collect(),
            ),
        );

        Ok(Value::Map(map))
    }

    /// Generate a prompt for LLM skill interpretation
    /// This can be used with an external LLM to interpret user intent
    pub fn generate_interpretation_prompt(&self, skill: &Skill, user_input: &str) -> String {
        let mut prompt = format!(
            "You are executing the skill: {}\n\nDescription: {}\n\n",
            skill.name, skill.description
        );

        prompt.push_str("Instructions:\n");
        prompt.push_str(&skill.instructions);
        prompt.push_str("\n\n");

        if !skill.examples.is_empty() {
            prompt.push_str("Examples:\n");
            for example in &skill.examples {
                prompt.push_str(&format!(
                    "- Input: \"{}\"\n  Capability: {}\n  Params: {}\n",
                    example.input, example.capability, example.params
                ));
            }
            prompt.push_str("\n");
        }

        prompt.push_str(&format!(
            "Available capabilities: {}\n\n",
            skill.capabilities.join(", ")
        ));

        prompt.push_str(&format!("User input: \"{}\"\n\n", user_input));
        prompt.push_str("Respond with the capability to call and the parameters in JSON format.\n");

        prompt
    }

    /// Load a skill from URL and register it
    pub async fn load_and_register(
        &mut self,
        url: &str,
    ) -> Result<crate::skills::LoadedSkillInfo, SkillError> {
        // Load skill from URL
        let mut loaded = crate::skills::load_skill_from_url(url)
            .await
            .map_err(|e| SkillError::Execution(format!("Failed to load skill: {}", e)))?;

        // Register the skill operations as capabilities
        let registered_cap_ids = self
            .register_skill_capabilities(&loaded.skill, Some(&loaded.source_url))
            .await?;

        // Update loaded info
        loaded.capabilities_to_register = registered_cap_ids;

        // Register the skill in our local map
        self.register_skill(loaded.skill.clone());

        Ok(loaded)
    }

    /// Register a skill's operations as dynamic capabilities in the marketplace
    pub async fn register_skill_capabilities(
        &self,
        skill: &Skill,
        source_url: Option<&str>,
    ) -> Result<Vec<String>, SkillError> {
        let mut registered_ids = Vec::new();
        let primitive_mapper = PrimitiveMapper::new();
        let mut approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>> = None;

        for op in &skill.operations {
            let cap_id = format!("{}.{}", skill.id, op.name);
            let description = op.description.clone();
            let name = format!("{} ({})", skill.name, op.name);

            let mut metadata = HashMap::new();
            metadata.insert("skill_id".to_string(), skill.id.clone());
            metadata.insert("skill_operation".to_string(), op.name.clone());
            if let Some(url) = source_url {
                metadata.insert("skill_source_url".to_string(), url.to_string());
            }
            if let Some(cmd) = &op.command {
                metadata.insert("skill_command".to_string(), cmd.clone());
            }

            if let Some(cmd) = &op.command {
                if let Some((first_cmd, second_cmd)) = split_pipeline(cmd) {
                    if let Some(first_mapped) = primitive_mapper.map_command(first_cmd) {
                        if let Some(second_mapped) = primitive_mapper.map_command(second_cmd) {
                            if first_mapped.capability_id == "ccos.network.http-fetch"
                                && second_mapped.capability_id == "ccos.json.parse"
                            {
                                metadata.insert(
                                    "pipeline".to_string(),
                                    format!(
                                        "{}|{}",
                                        first_mapped.capability_id, second_mapped.capability_id
                                    ),
                                );
                                metadata.insert("pipeline_raw".to_string(), cmd.clone());

                                let marketplace = self.marketplace.clone();
                                let op_name = op.name.clone();
                                let skill_id = skill.id.clone();
                                let op_input_schema = op.input_schema.clone();
                                let first_cmd = first_cmd.to_string();
                                let skill_secrets = skill.secrets.clone();
                                let cap_id_for_handler = cap_id.clone();
                                let skill_cloned = skill.clone();
                                let source_url_cloned = source_url.map(|s| s.to_string());

                                let handler = Arc::new(move |inputs: &Value| {
                                    let marketplace = marketplace.clone();
                                    let op_name = op_name.clone();
                                    let skill_id = skill_id.clone();
                                    let op_input_schema = op_input_schema.clone();
                                    let first_cmd = first_cmd.clone();
                                    let primitive_mapper = PrimitiveMapper::new();
                                    let skill_secrets = skill_secrets.clone();
                                    let cap_id = cap_id_for_handler.clone();
                                    let skill = skill_cloned.clone();
                                    let source_url = source_url_cloned.clone();

                                    let inputs_cloned = inputs.clone();
                                    Box::pin(async move {
                                        // Check approval status before execution
                                        check_approval_status(&cap_id).await?;

                                        if let Some(schema) = op_input_schema.as_ref() {
                                            validate_skill_inputs(&inputs_cloned, schema, &cap_id)?;
                                        }

                                        let inputs = &inputs_cloned;
                                        let first_mapped = primitive_mapper
                                            .map_command(&first_cmd)
                                            .ok_or_else(|| {
                                                RuntimeError::Generic(format!(
                                                    "Operation {}.{} has no valid command mapping",
                                                    skill_id, op_name
                                                ))
                                            })?;

                                        let mut final_params = first_mapped.params.clone();
                                        log_skill_op(&cap_id, &skill_id, &op_name);

                                        // Inject secrets into headers
                                        inject_secrets_into_params(
                                            &mut final_params,
                                            &skill_secrets,
                                        );

                                        // Resolve relative URLs and replace placeholders
                                        resolve_relative_url(
                                            &mut final_params,
                                            &skill,
                                            source_url.as_deref(),
                                        );
                                        template_replace_secrets(&mut final_params, &skill_secrets);

                                        let inputs_json = rtfs_value_to_json(inputs)?;
                                        match inputs_json {
                                            serde_json::Value::Object(obj) => {
                                                for (k, v) in obj {
                                                    // Special case: if "body" is an object/array,
                                                    // serialize it to a JSON string for HTTP POST.
                                                    if k == "body"
                                                        && (v.is_object() || v.is_array())
                                                    {
                                                        final_params.insert(
                                                            k,
                                                            serde_json::Value::String(
                                                                v.to_string(),
                                                            ),
                                                        );
                                                    } else {
                                                        final_params.insert(k, v);
                                                    }
                                                }
                                            }
                                            serde_json::Value::Null => {}
                                            other => {
                                                final_params.insert("input".to_string(), other);
                                            }
                                        }

                                        ensure_json_body_for_write(&mut final_params);
                                        log_skill_params(
                                            &first_mapped.capability_id,
                                            &final_params,
                                        );

                                        let rtfs_params_json = serde_json::Value::Object(
                                            final_params
                                                .into_iter()
                                                .collect::<serde_json::Map<_, _>>(),
                                        );
                                        let rtfs_params = json_to_rtfs_value(&rtfs_params_json)?;

                                        let fetch_result = marketplace
                                            .execute_capability(
                                                &first_mapped.capability_id,
                                                &rtfs_params,
                                            )
                                            .await?;

                                        let body = match fetch_result {
                                            Value::Map(m) => {
                                                let key = MapKey::String("body".to_string());
                                                match m.get(&key) {
                                                    Some(Value::String(s)) => s.clone(),
                                                    Some(v) => rtfs_value_to_json(v)?.to_string(),
                                                    None => {
                                                        return Err(RuntimeError::Generic(
                                                            "http-fetch result missing body"
                                                                .to_string(),
                                                        ))
                                                    }
                                                }
                                            }
                                            Value::String(s) => s,
                                            other => rtfs_value_to_json(&other)?.to_string(),
                                        };

                                        let parse_args = Value::List(vec![Value::String(body)]);
                                        marketplace
                                            .execute_capability("ccos.json.parse", &parse_args)
                                            .await
                                    })
                                        as BoxFuture<'static, RuntimeResult<Value>>
                                });

                                let manifest = CapabilityManifest {
                                    id: cap_id.clone(),
                                    name,
                                    description,
                                    provider: ProviderType::Native(NativeCapability {
                                        handler,
                                        security_level: "default".to_string(),
                                        metadata: HashMap::new(),
                                    }),
                                    version: "1.0.0".to_string(),
                                    input_schema: op.input_schema.clone(),
                                    output_schema: op.output_schema.clone(),
                                    attestation: None,
                                    provenance: None,
                                    permissions: vec![],
                                    effects: vec![],
                                    metadata,
                                    agent_metadata: None,
                                    domains: Vec::new(),
                                    categories: Vec::new(),
                                    effect_type: EffectType::Effectful,
                                };

                                self.marketplace
                                    .register_capability_manifest(manifest)
                                    .await
                                    .map_err(|e| {
                                        SkillError::Execution(format!(
                                            "Failed to register capability: {}",
                                            e
                                        ))
                                    })?;

                                if skill.approval.required || !skill.secrets.is_empty() {
                                    ensure_skill_approvals(
                                        &mut approval_queue,
                                        skill,
                                        &cap_id,
                                        source_url,
                                        cmd,
                                        false,
                                    )
                                    .await?;
                                }

                                registered_ids.push(cap_id);
                                continue;
                            }
                        }
                    }
                }
                if let Some(mapped) = primitive_mapper.map_command(cmd) {
                    metadata.insert("delegated_to".to_string(), mapped.capability_id.clone());

                    // Create handler for this capability
                    let marketplace = self.marketplace.clone();
                    let command = op.command.clone();
                    let op_name = op.name.clone();
                    let skill_id = skill.id.clone();
                    let op_input_schema = op.input_schema.clone();
                    let skill_secrets = skill.secrets.clone();
                    let cap_id_for_handler = cap_id.clone();
                    let skill_cloned = skill.clone();
                    let source_url_cloned = source_url.map(|s| s.to_string());

                    let handler = Arc::new(move |inputs: &Value| {
                        let marketplace = marketplace.clone();
                        let command = command.clone();
                        let op_name = op_name.clone();
                        let skill_id = skill_id.clone();
                        let op_input_schema = op_input_schema.clone();
                        let primitive_mapper = PrimitiveMapper::new();
                        let skill_secrets = skill_secrets.clone();
                        let cap_id = cap_id_for_handler.clone();
                        let skill = skill_cloned.clone();
                        let source_url = source_url_cloned.clone();

                        let inputs_cloned = inputs.clone();
                        Box::pin(async move {
                            // Check approval status before execution
                            check_approval_status(&cap_id).await?;

                            if let Some(schema) = op_input_schema.as_ref() {
                                validate_skill_inputs(&inputs_cloned, schema, &cap_id)?;
                            }

                            let inputs = &inputs_cloned;
                            // 1. If we have a command, map it to a primitive
                            if let Some(cmd) = command {
                                if let Some(mapped) = primitive_mapper.map_command(&cmd) {
                                    log_skill_op(&cap_id, &skill_id, &op_name);
                                    // Merge inputs with mapped params
                                    let mut final_params = mapped.params.clone();

                                    // Inject secrets into headers
                                    inject_secrets_into_params(&mut final_params, &skill_secrets);

                                    // Resolve relative URLs and replace placeholders
                                    resolve_relative_url(
                                        &mut final_params,
                                        &skill,
                                        source_url.as_deref(),
                                    );
                                    template_replace_secrets(&mut final_params, &skill_secrets);

                                    // Merge user-provided inputs (RTFS) into mapped params (JSON)
                                    // using the shared conversion utility.
                                    let inputs_json = rtfs_value_to_json(inputs)?;
                                    match inputs_json {
                                        serde_json::Value::Object(obj) => {
                                            for (k, v) in obj {
                                                // Special case: if "body" is an object/array,
                                                // serialize it to a JSON string for HTTP POST.
                                                if k == "body" && (v.is_object() || v.is_array()) {
                                                    final_params.insert(
                                                        k,
                                                        serde_json::Value::String(v.to_string()),
                                                    );
                                                } else {
                                                    final_params.insert(k, v);
                                                }
                                            }
                                        }
                                        serde_json::Value::Null => {}
                                        other => {
                                            // If the caller passed a non-map input, keep it under "input"
                                            // so the underlying capability can still consume it.
                                            final_params.insert("input".to_string(), other);
                                        }
                                    }

                                    ensure_json_body_for_write(&mut final_params);
                                    log_skill_params(&mapped.capability_id, &final_params);

                                    // Convert merged params back to RTFS Value
                                    let rtfs_params_json = serde_json::Value::Object(
                                        final_params.into_iter().collect::<serde_json::Map<_, _>>(),
                                    );
                                    let rtfs_params = json_to_rtfs_value(&rtfs_params_json)?;

                                    // Execute underlying capability
                                    return marketplace
                                        .execute_capability(&mapped.capability_id, &rtfs_params)
                                        .await
                                        .map_err(|e| e.into());
                                }
                            }

                            Err(RuntimeError::Generic(format!(
                                "Operation {}.{} has no valid command mapping",
                                skill_id, op_name
                            )))
                        }) as BoxFuture<'static, RuntimeResult<Value>>
                    });

                    let manifest = CapabilityManifest {
                        id: cap_id.clone(),
                        name,
                        description,
                        provider: ProviderType::Native(NativeCapability {
                            handler,
                            security_level: "default".to_string(),
                            metadata: HashMap::new(),
                        }),
                        version: "1.0.0".to_string(),
                        input_schema: op.input_schema.clone(),
                        output_schema: op.output_schema.clone(),
                        attestation: None,
                        provenance: None,
                        permissions: vec![],
                        effects: vec![],
                        metadata,
                        agent_metadata: None,
                        domains: Vec::new(),
                        categories: Vec::new(),
                        effect_type: EffectType::Effectful,
                    };

                    self.marketplace
                        .register_capability_manifest(manifest)
                        .await
                        .map_err(|e| {
                            SkillError::Execution(format!("Failed to register capability: {}", e))
                        })?;

                    if skill.approval.required || !skill.secrets.is_empty() {
                        ensure_skill_approvals(
                            &mut approval_queue,
                            skill,
                            &cap_id,
                            source_url,
                            cmd,
                            false,
                        )
                        .await?;
                    }

                    registered_ids.push(cap_id);
                    continue;
                }

                // Unknown command -> sandboxed capability
                metadata.insert("sandbox_reason".to_string(), "unknown_tool".to_string());
                let manifest = CapabilityManifest {
                    id: cap_id.clone(),
                    name,
                    description,
                    provider: ProviderType::Sandboxed(SandboxedCapability {
                        runtime: "shell".to_string(),
                        source: cmd.clone(),
                        entry_point: None,
                        provider: Some("process".to_string()),
                        runtime_spec: None,
                        network_policy: None,
                        filesystem: None,
                        resources: None,
                        secrets: Vec::new(),
                    }),
                    version: "1.0.0".to_string(),
                    input_schema: op.input_schema.clone(),
                    output_schema: op.output_schema.clone(),
                    attestation: None,
                    provenance: None,
                    permissions: vec![],
                    effects: vec![],
                    metadata,
                    agent_metadata: None,
                    domains: Vec::new(),
                    categories: Vec::new(),
                    effect_type: EffectType::Effectful,
                };

                self.marketplace
                    .register_capability_manifest(manifest)
                    .await
                    .map_err(|e| {
                        SkillError::Execution(format!("Failed to register capability: {}", e))
                    })?;

                ensure_skill_approvals(&mut approval_queue, skill, &cap_id, source_url, cmd, true)
                    .await?;

                registered_ids.push(cap_id);
                continue;
            }

            return Err(SkillError::Validation(format!(
                "Operation {}.{} has no command to map",
                skill.id, op.name
            )));
        }

        Ok(registered_ids)
    }
}

fn split_pipeline(command: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = command.split('|').map(|s| s.trim()).collect();
    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

/// Check if a capability has pending approvals that block execution.
/// Returns Ok(()) if approved or no approval required, Err with reason if blocked.
async fn check_approval_status(capability_id: &str) -> RuntimeResult<()> {
    let workspace_root = get_workspace_root();
    let approval_base = if workspace_root.ends_with("config") {
        workspace_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(workspace_root.clone())
    } else {
        workspace_root.clone()
    };
    let storage_path =
        approval_base.join(&rtfs::config::AgentConfig::from_env().storage.approvals_dir);

    let storage = match FileApprovalStorage::new(storage_path) {
        Ok(s) => s,
        Err(_) => return Ok(()), // No storage = no approvals enforced
    };
    let queue = UnifiedApprovalQueue::new(std::sync::Arc::new(storage));

    // Check for pending EffectApproval for this capability
    let pending = queue
        .list(ApprovalFilter {
            category_type: Some("EffectApproval".to_string()),
            status_pending: Some(true),
            ..Default::default()
        })
        .await
        .unwrap_or_default();

    for req in pending {
        if let ApprovalCategory::EffectApproval {
            capability_id: ref cap_id,
            ..
        } = req.category
        {
            if cap_id == capability_id {
                return Err(RuntimeError::Generic(format!(
                    "Capability '{}' requires approval before execution (pending approval ID: {})",
                    capability_id, req.id
                )));
            }
        }
    }

    // Check for pending SecretRequired for this capability
    let pending_secrets = queue
        .list(ApprovalFilter {
            category_type: Some("SecretRequired".to_string()),
            status_pending: Some(true),
            ..Default::default()
        })
        .await
        .unwrap_or_default();

    for req in pending_secrets {
        if let ApprovalCategory::SecretRequired {
            capability_id: ref cap_id,
            secret_type,
            ..
        } = req.category
        {
            if cap_id == capability_id {
                return Err(RuntimeError::Generic(format!(
                    "Capability '{}' requires secret '{}' approval before execution (pending approval ID: {})",
                    capability_id, secret_type, req.id
                )));
            }
        }
    }

    Ok(())
}

fn resolve_relative_url(
    params: &mut HashMap<String, serde_json::Value>,
    skill: &Skill,
    source_url: Option<&str>,
) {
    if let Some(url_val) = params.get_mut("url") {
        if let Some(url_str) = url_val.as_str() {
            if url_str.starts_with('/') {
                // Resolve base URL
                let base_str: Option<String> = skill
                    .metadata
                    .get("api_base")
                    .map(|s| s.to_string())
                    .or_else(|| {
                        source_url.and_then(|su| {
                            let parts: Vec<&str> = su.split('/').collect();
                            if parts.len() >= 3 {
                                // Extract protocol and host (e.g. http://localhost:8765)
                                Some(parts[0..3].join("/"))
                            } else {
                                None
                            }
                        })
                    });

                if let Some(base_url) = base_str {
                    let mut final_base = base_url.to_string();
                    if final_base.ends_with('/') {
                        final_base.pop();
                    }
                    *url_val = serde_json::Value::String(format!("{}{}", final_base, url_str));
                }
            }
        }
    }
}

fn template_replace_secrets(params: &mut HashMap<String, serde_json::Value>, secrets: &[String]) {
    let secret_store = SecretStore::new(Some(get_workspace_root())).ok();
    let secret_store = match secret_store {
        Some(s) => s,
        None => return,
    };

    let mut values_to_replace = Vec::new();
    for secret_name in secrets {
        if let Some(secret_value) = secret_store.get(secret_name) {
            values_to_replace.push((format!("{{{}}}", secret_name), secret_value.clone()));
            values_to_replace.push((format!("${{{}}}", secret_name), secret_value.clone()));
            values_to_replace.push((format!("${}", secret_name), secret_value.clone()));
        }
    }

    if values_to_replace.is_empty() {
        return;
    }

    // Replace in all string values (including headers)
    for value in params.values_mut() {
        replace_in_json_value(value, &values_to_replace);
    }
}

fn replace_in_json_value(value: &mut serde_json::Value, replacements: &[(String, String)]) {
    match value {
        serde_json::Value::String(s) => {
            for (placeholder, secret) in replacements {
                if s.contains(placeholder) {
                    *s = s.replace(placeholder, secret);
                }
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                replace_in_json_value(v, replacements);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                replace_in_json_value(v, replacements);
            }
        }
        _ => {}
    }
}

/// Inject secrets into HTTP params (headers) for a skill.
/// Looks up secrets from SecretStore and adds Authorization header if found.
fn inject_secrets_into_params(params: &mut HashMap<String, serde_json::Value>, secrets: &[String]) {
    let secret_store = SecretStore::new(Some(get_workspace_root())).ok();
    let secret_store = match secret_store {
        Some(s) => s,
        None => return,
    };

    for secret_name in secrets {
        if let Some(secret_value) = secret_store.get(secret_name) {
            // Inject as Authorization Bearer header by default
            let headers = params
                .entry("headers".to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

            if let serde_json::Value::Object(ref mut map) = headers {
                // Check if secret name looks like an API key pattern
                if secret_name.to_uppercase().contains("API_KEY")
                    || secret_name.to_uppercase().contains("TOKEN")
                    || secret_name.to_uppercase().contains("AUTH")
                {
                    // Use Bearer token format
                    map.insert(
                        "Authorization".to_string(),
                        serde_json::Value::String(format!("Bearer {}", secret_value)),
                    );
                } else {
                    // Use X-API-Key header
                    map.insert(
                        "X-API-Key".to_string(),
                        serde_json::Value::String(secret_value),
                    );
                }
            }
        }
    }
}

fn validate_skill_inputs(
    inputs: &Value,
    schema: &rtfs::ast::TypeExpr,
    cap_id: &str,
) -> RuntimeResult<()> {
    if let Some(missing) = missing_required_inputs(inputs, schema) {
        if !missing.is_empty() {
            return Err(RuntimeError::Generic(format!(
                "PROMPT_MISSING_INPUT: Missing required inputs for {}: {}. Please provide these values to continue.",
                cap_id,
                missing.join(", ")
            )));
        }
    }

    let validator = TypeValidator::default();
    let config = TypeCheckingConfig::default();
    let context = VerificationContext::capability_boundary(cap_id);
    validator
        .validate_with_config(inputs, schema, &config, &context)
        .map_err(|e| {
            RuntimeError::Generic(format!("Input validation failed for {}: {}", cap_id, e))
        })?;
    Ok(())
}

fn missing_required_inputs(inputs: &Value, schema: &rtfs::ast::TypeExpr) -> Option<Vec<String>> {
    let schema_json = schema.to_json().ok()?;
    let required = schema_json
        .get("required")
        .and_then(|v| v.as_array())?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    if required.is_empty() {
        return Some(vec![]);
    }
    let inputs_json = rtfs_value_to_json(inputs).ok()?;
    let present = inputs_json
        .as_object()
        .map(|m| m.keys().cloned().collect::<std::collections::HashSet<_>>())
        .unwrap_or_default();
    let missing = required
        .into_iter()
        .filter(|k| !present.contains(k))
        .collect::<Vec<_>>();
    Some(missing)
}

fn log_skill_params(capability_id: &str, params: &HashMap<String, serde_json::Value>) {
    let map: serde_json::Map<String, serde_json::Value> =
        params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let json = serde_json::Value::Object(map);
    let redacted = redact_json_for_logs(&json);
    info!("Skill call: {} params={}", capability_id, redacted);
}

fn log_skill_op(capability_id: &str, skill_id: &str, op_name: &str) {
    info!(
        "Skill operation: skill_id={}, op={}, capability={}",
        skill_id, op_name, capability_id
    );
}

fn ensure_json_body_for_write(params: &mut HashMap<String, serde_json::Value>) {
    let Some(method) = params.get("method").and_then(|v| v.as_str()) else {
        return;
    };
    let method = method.to_ascii_uppercase();
    let is_write = matches!(method.as_str(), "POST" | "PUT" | "PATCH");
    if !is_write {
        return;
    }
    if !params.contains_key("body") {
        params.insert(
            "body".to_string(),
            serde_json::Value::String("{}".to_string()),
        );
    }
    let body_is_json = params
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| {
            let trimmed = s.trim();
            trimmed.starts_with('{') || trimmed.starts_with('[')
        })
        .unwrap_or(false);
    if !body_is_json {
        return;
    }
    let headers = params
        .entry("headers".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let serde_json::Value::Object(map) = headers {
        let has_ct = map.keys().any(|k| k.eq_ignore_ascii_case("content-type"));
        if !has_ct {
            map.insert(
                "Content-Type".to_string(),
                serde_json::Value::String("application/json".to_string()),
            );
        }
    }
}

async fn ensure_skill_approvals(
    approval_queue: &mut Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    skill: &Skill,
    capability_id: &str,
    source_url: Option<&str>,
    command: &str,
    sandboxed_unknown: bool,
) -> Result<(), SkillError> {
    let queue = if let Some(queue) = approval_queue.clone() {
        queue
    } else {
        let workspace_root = get_workspace_root();
        let approval_base = if workspace_root.ends_with("config") {
            workspace_root
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(workspace_root.clone())
        } else {
            workspace_root.clone()
        };
        let storage_path =
            approval_base.join(&rtfs::config::AgentConfig::from_env().storage.approvals_dir);
        let storage = FileApprovalStorage::new(storage_path).map_err(|e| {
            SkillError::Execution(format!("Failed to init approval storage: {}", e))
        })?;
        let queue = UnifiedApprovalQueue::new(std::sync::Arc::new(storage));
        *approval_queue = Some(queue.clone());
        queue
    };

    let mut context = format!(
        "skill_id={} skill_name={} operation={}",
        skill.id, skill.name, capability_id
    );
    if let Some(url) = source_url {
        context.push_str(&format!(" source_url={}", url));
    }
    context.push_str(&format!(" command={}", command));

    if !skill.secrets.is_empty() {
        for secret in &skill.secrets {
            let request = ApprovalRequest::new(
                ApprovalCategory::SecretRequired {
                    capability_id: capability_id.to_string(),
                    secret_type: secret.clone(),
                    description: format!("Skill '{}' requires secret '{}'", skill.name, secret),
                },
                RiskAssessment {
                    level: RiskLevel::Medium,
                    reasons: vec!["Skill requires secret".to_string()],
                },
                24,
                Some(context.clone()),
            );
            queue.add(request).await.map_err(|e| {
                SkillError::Execution(format!("Failed to enqueue secret approval: {}", e))
            })?;
        }
    }

    if skill.approval.required {
        let effects = if skill.effects.is_empty() {
            vec!["skill.approval".to_string()]
        } else {
            skill.effects.clone()
        };
        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id: capability_id.to_string(),
                effects,
                intent_description: format!("Skill '{}' requires approval", skill.name),
            },
            RiskAssessment {
                level: RiskLevel::Medium,
                reasons: vec!["Skill flagged for approval".to_string()],
            },
            24,
            Some(context.clone()),
        );
        queue
            .add(request)
            .await
            .map_err(|e| SkillError::Execution(format!("Failed to enqueue approval: {}", e)))?;
    }

    if sandboxed_unknown {
        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id: capability_id.to_string(),
                effects: vec!["sandbox".to_string()],
                intent_description: "Unknown tool requires sandboxed execution".to_string(),
            },
            RiskAssessment {
                level: RiskLevel::High,
                reasons: vec!["Unknown tool routed to sandbox".to_string()],
            },
            24,
            Some(context),
        );
        queue.add(request).await.map_err(|e| {
            SkillError::Execution(format!("Failed to enqueue sandbox approval: {}", e))
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::storage_file::FileApprovalStorage;
    use crate::approval::{ApprovalCategory, ApprovalFilter, UnifiedApprovalQueue};
    use crate::capabilities::registry::CapabilityRegistry;
    use crate::skills::types::SkillOperation;
    use rtfs::ast::TypeExpr;
    use rtfs::runtime::values::Value;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_skill_registration() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let mut mapper = SkillMapper::new(marketplace);

        let skill = Skill::new(
            "test-skill",
            "Test Skill",
            "A test skill",
            vec!["test.cap".to_string()],
            "Test instructions",
        );

        mapper.register_skill(skill);
        assert!(mapper.get_skill("test-skill").is_some());
        assert_eq!(mapper.list_skills().len(), 1);
    }

    #[test]
    fn test_intent_builder() {
        let intent = Intent::new("Find coffee shops near me")
            .with_param("location", Value::String("current".to_string()));

        assert_eq!(intent.description, "Find coffee shops near me");
        assert!(intent.params.contains_key("location"));
    }

    #[tokio::test]
    async fn test_generate_prompt() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let mapper = SkillMapper::new(marketplace);

        let skill = Skill::new(
            "search-places",
            "Search Places",
            "Find nearby places",
            vec!["maps.search".to_string()],
            "Use this to find restaurants and shops.",
        );

        let prompt = mapper.generate_interpretation_prompt(&skill, "Find pizza near me");
        assert!(prompt.contains("Search Places"));
        assert!(prompt.contains("Find pizza near me"));
        assert!(prompt.contains("maps.search"));
    }

    #[tokio::test]
    async fn test_skill_registration_enqueues_approvals() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let mapper = SkillMapper::new(marketplace);

        let skill_id = format!("skill-approval-test-{}", uuid::Uuid::new_v4());
        let mut skill = Skill::new(
            &skill_id,
            "Skill Approval Test",
            "A skill requiring approvals",
            vec![],
            "Test instructions",
        );
        skill.secrets = vec!["TEST_SECRET".to_string()];
        skill.approval.required = true;
        skill.operations = vec![SkillOperation {
            name: "convert".to_string(),
            description: "Convert media".to_string(),
            endpoint: None,
            method: None,
            command: Some("ffmpeg -i input.mp4 output.avi".to_string()),
            runtime: None,
            input_schema: None,
            output_schema: None,
        }];

        let cap_ids = mapper
            .register_skill_capabilities(&skill, Some("https://example.com/skill.md"))
            .await
            .expect("Skill capability registration should succeed");

        let cap_id = cap_ids.first().expect("Expected one capability").clone();

        let approvals_dir =
            get_workspace_root().join(&rtfs::config::AgentConfig::from_env().storage.approvals_dir);
        let storage =
            FileApprovalStorage::new(approvals_dir).expect("Approval storage should initialize");
        let queue = UnifiedApprovalQueue::new(Arc::new(storage));

        let pending = queue
            .list(ApprovalFilter::pending())
            .await
            .expect("Should list pending approvals");

        let mut found_secret = false;
        let mut found_skill_approval = false;
        let mut found_sandbox = false;

        for request in pending {
            match request.category {
                ApprovalCategory::SecretRequired {
                    capability_id,
                    secret_type,
                    ..
                } if capability_id == cap_id && secret_type == "TEST_SECRET" => {
                    found_secret = true;
                }
                ApprovalCategory::EffectApproval {
                    capability_id,
                    effects,
                    ..
                } if capability_id == cap_id && effects.iter().any(|e| e == "skill.approval") => {
                    found_skill_approval = true;
                }
                ApprovalCategory::EffectApproval {
                    capability_id,
                    effects,
                    ..
                } if capability_id == cap_id && effects.iter().any(|e| e == "sandbox") => {
                    found_sandbox = true;
                }
                _ => {}
            }
        }

        assert!(found_secret, "Secret approval should be enqueued");
        assert!(found_skill_approval, "Skill approval should be enqueued");
        assert!(found_sandbox, "Sandbox approval should be enqueued");
    }

    #[test]
    fn test_missing_required_inputs() {
        // Create a map type with one required and one optional field
        let type_str = "[:map [:name :string] [:age :int ?]]";
        let schema = TypeExpr::from_str(type_str).unwrap();

        // Case 1: All required present
        let mut inputs_map = HashMap::new();
        inputs_map.insert(
            MapKey::String("name".to_string()),
            Value::String("Alice".to_string()),
        );
        let inputs = Value::Map(inputs_map);
        let missing = missing_required_inputs(&inputs, &schema).unwrap();
        assert!(missing.is_empty());

        // Case 2: Required missing
        let inputs = Value::Map(HashMap::new());
        let missing = missing_required_inputs(&inputs, &schema).unwrap();
        assert_eq!(missing, vec!["name".to_string()]);
    }

    #[test]
    fn test_validate_skill_inputs_prompt() {
        let type_str = "[:map [:query :string]]";
        let schema = TypeExpr::from_str(type_str).unwrap();
        let inputs = Value::Map(HashMap::new());

        let result = validate_skill_inputs(&inputs, &schema, "test-cap");
        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert!(err.contains("PROMPT_MISSING_INPUT"));
        assert!(err.contains("query"));
    }
}
