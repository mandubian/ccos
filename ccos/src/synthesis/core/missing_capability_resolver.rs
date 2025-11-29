//! Missing Capability Resolution System
//!
//! This module implements Phase 2 of the missing capability resolution plan:
//! - Runtime trap for missing capability errors
//! - Resolution queue for background processing
//! - Integration with marketplace discovery

use crate::arbiter::prompt::{FilePromptStore, PromptManager};
use crate::arbiter::DelegatingArbiter;
use crate::capability_marketplace::types::{
    CapabilityKind, CapabilityManifest, CapabilityQuery, LocalCapability, ProviderType,
};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::checkpoint_archive::CheckpointArchive;
use crate::discovery::capability_matcher::{
    calculate_action_verb_match_score, calculate_description_match_score, extract_action_verbs,
};
use crate::discovery::need_extractor::CapabilityNeed;
use crate::rtfs_bridge::expression_to_pretty_rtfs_string;
use crate::rtfs_bridge::expression_to_rtfs_string;
use super::feature_flags::{FeatureFlagChecker, MissingCapabilityConfig};
use super::missing_capability_strategies::{
    MissingCapabilityStrategy, MissingCapabilityStrategyConfig, PureRtfsGenerationStrategy,
    UserInteractionStrategy, ExternalLlmHintStrategy, ServiceDiscoveryHintStrategy,
};
use super::schema_serializer::type_expr_to_rtfs_compact;
use crate::synthesis::dialogue::capability_synthesizer::MultiCapabilityEndpoint;
use crate::synthesis::primitives::executor::RestrictedRtfsExecutor;
use crate::synthesis::runtime::server_trust::{
    create_default_trust_registry, ServerCandidate, ServerSelectionHandler, ServerTrustRegistry,
};
use crate::utils::value_conversion;
use rtfs::ast::TypeExpr;
use rtfs::ast::{
    Expression, Keyword as RtfsKeyword, Literal, MapKey, MapTypeEntry, PrimitiveType,
    Symbol as RtfsSymbol,
};
use rtfs::parser::{parse_expression, parse_type_expression};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap as StdHashMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use once_cell::sync::Lazy;
use regex::Regex;

// Quiet console logging helper: when CCOS_QUIET_RESOLVER=1|true|on, suppress direct eprintln! noise
macro_rules! quiet_eprintln {
    ($($arg:tt)*) => {{
        let quiet = std::env::var("CCOS_QUIET_RESOLVER")
            .map(|v| { let v = v.to_lowercase(); v == "1" || v == "true" || v == "on" })
            .unwrap_or(false);
        if !quiet {
            eprintln!($($arg)*);
        }
    }};
}

const CAPABILITY_PROMPT_ID: &str = "capability_synthesis";
const CAPABILITY_PROMPT_VERSION: &str = "v1";

static CAPABILITY_PROMPT_MANAGER: Lazy<PromptManager<FilePromptStore>> = Lazy::new(|| {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/prompts/arbiter");
    PromptManager::new(FilePromptStore::new(&base_dir))
});

static CODE_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"```(?:rtfs|lisp|scheme)?\s*([\s\S]*?)```").unwrap());

static PRELUDE_HELPERS: &[&str] = &[
    "- println, log, tool/log, tool/time-ms",
    "- safe arithmetic helpers: +, -, *, /, zero?, =",
    "- collection helpers: map, filter, reduce, sort-by, group-by",
    "- string helpers: str, string-lower, string-contains, concat",
];

/// Curated overrides file format for MCP server discovery
#[derive(Debug, Clone, Deserialize)]
struct CuratedOverrides {
    pub entries: Vec<CuratedEntry>,
}

/// One curated entry with match patterns and the MCP server descriptor
#[derive(Debug, Clone, Deserialize)]
struct CuratedEntry {
    pub matches: Vec<String>,
    pub server: crate::mcp::registry::McpServer,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ToolAliasFile {
    pub entries: Vec<ToolAliasRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolAliasRecord {
    #[serde(rename = "capability")]
    pub capability_pattern: String,
    pub server_name: String,
    pub server_url: String,
    pub tool_name: String,
    #[serde(default)]
    pub input_remap: HashMap<String, String>,
}

#[derive(Debug)]
struct ToolAliasStore {
    path: PathBuf,
    records: HashMap<String, ToolAliasRecord>,
}

#[derive(Debug, Clone)]
struct ToolSelectionResult {
    tool_name: String,
    input_remap: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct ToolPromptCandidate {
    index: usize,
    tool_name: String,
    description: String,
    input_keys: Vec<String>,
    score: f64,
}

impl ToolAliasStore {
    fn load_default() -> Self {
        let default_path = PathBuf::from("../capabilities/mcp/aliases.json");
        Self::load(default_path)
    }

    fn build_mcp_auth_headers(server_name: &str) -> Option<HashMap<String, String>> {
        Self::get_mcp_auth_token(server_name).map(|token| {
            let mut headers = HashMap::new();
            // If the provided token already includes a scheme (e.g. "Bearer ..."),
            // use it verbatim. Otherwise, prefix with "Bearer ". This makes the
            // environment variable flexible and avoids double-prefixing when users
            // supply a full Authorization header value.
            let auth_value = if token.to_lowercase().starts_with("bearer ")
                || token.to_lowercase().starts_with("token ")
                || token.to_lowercase().starts_with("basic ")
            {
                token
            } else {
                format!("Bearer {}", token)
            };

            headers.insert("Authorization".to_string(), auth_value);
            headers
        })
    }

    fn get_mcp_auth_token(server_name: &str) -> Option<String> {
        let namespace = if let Some(slash_pos) = server_name.find('/') {
            &server_name[..slash_pos]
        } else {
            server_name
        };

        let normalized_namespace = namespace.replace('-', "_").to_uppercase();
        let server_specific_var = format!("{}_MCP_TOKEN", normalized_namespace);

        if let Ok(token) = std::env::var(&server_specific_var) {
            if !token.is_empty() {
                return Some(token);
            }
        }

        if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }

        None
    }

    fn suggest_mcp_token_env_var(server_name: &str) -> String {
        let namespace = if let Some(slash_pos) = server_name.find('/') {
            &server_name[..slash_pos]
        } else {
            server_name
        };

        let normalized = namespace.replace('-', "_").to_uppercase();
        format!("{}_MCP_TOKEN", normalized)
    }

    async fn materialize_alias(
        alias: &ToolAliasRecord,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        let auth_headers = Self::build_mcp_auth_headers(&alias.server_name);
        let introspector = crate::synthesis::mcp_introspector::MCPIntrospector::new();

        let introspection = introspector
            .introspect_mcp_server_with_auth(
                &alias.server_url,
                &alias.server_name,
                auth_headers.clone(),
            )
            .await?;

        let capabilities = introspector.create_capabilities_from_mcp(&introspection)?;
        let mut manifest = capabilities.into_iter().find(|manifest| {
            manifest
                .metadata
                .get("mcp_tool_name")
                .map(|name| name == &alias.tool_name)
                .unwrap_or(false)
        });

        if let Some(ref mut manifest) = manifest {
            if !alias.input_remap.is_empty() {
                if let Ok(remap_json) = serde_json::to_string(&alias.input_remap) {
                    manifest
                        .metadata
                        .insert("mcp_input_remap".to_string(), remap_json);
                }
            }
            manifest
                .metadata
                .insert("resolution_source".to_string(), "alias".to_string());
        }

        Ok(manifest)
    }

    fn load(path: PathBuf) -> Self {
        let mut store = Self {
            path,
            records: HashMap::new(),
        };

        if let Ok(contents) = fs::read_to_string(&store.path) {
            if let Ok(file) = serde_json::from_str::<ToolAliasFile>(&contents) {
                for entry in file.entries {
                    let key = Self::normalize_key(&entry.capability_pattern);
                    store.records.insert(key, entry);
                }
            }
        }

        store
    }

    fn normalize_key(value: &str) -> String {
        value.trim().to_ascii_lowercase()
    }

    fn lookup(&self, capability_id: &str) -> Option<ToolAliasRecord> {
        let key = Self::normalize_key(capability_id);
        self.records.get(&key).cloned()
    }

    fn insert(&mut self, entry: ToolAliasRecord) -> RuntimeResult<()> {
        let key = Self::normalize_key(&entry.capability_pattern);
        self.records.insert(key, entry);
        self.persist()
    }

    fn remove(&mut self, capability_pattern: &str) -> RuntimeResult<()> {
        let key = Self::normalize_key(capability_pattern);
        if self.records.remove(&key).is_some() {
            self.persist()
        } else {
            Ok(())
        }
    }

    fn persist(&self) -> RuntimeResult<()> {
        let mut entries: Vec<ToolAliasRecord> = self.records.values().cloned().collect();
        entries.sort_by(|a, b| a.capability_pattern.cmp(&b.capability_pattern));

        let file = ToolAliasFile { entries };
        let json = serde_json::to_string_pretty(&file).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize tool aliases: {}", e))
        })?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to create alias directory {:?}: {}",
                    parent, e
                ))
            })?;
        }

        fs::write(&self.path, json).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to persist tool aliases to {:?}: {}",
                self.path, e
            ))
        })
    }
}

/// Represents a missing capability that needs resolution
#[derive(Debug, Clone)]
pub struct MissingCapabilityRequest {
    /// The capability ID that was requested but not found
    pub capability_id: String,
    /// Arguments that were passed to the capability (for context)
    pub arguments: Vec<rtfs::runtime::values::Value>,
    /// Context from the execution (plan_id, intent_id, etc.)
    pub context: HashMap<String, String>,
    /// Timestamp when the request was created
    pub requested_at: std::time::SystemTime,
    /// Number of resolution attempts made
    pub attempt_count: u32,
}

/// Result of a capability resolution attempt
#[derive(Debug, Clone)]
pub enum ResolutionResult {
    /// Capability was successfully resolved and registered
    Resolved {
        capability_id: String,
        resolution_method: String,
        provider_info: Option<String>,
    },
    /// Resolution failed and should be retried later
    Failed {
        capability_id: String,
        reason: String,
        retry_after: Option<std::time::Duration>,
    },
    /// Resolution permanently failed (e.g., invalid capability ID)
    PermanentlyFailed {
        capability_id: String,
        reason: String,
    },
}

/// An MCP server with its relevance score
#[derive(Debug, Clone)]
pub struct RankedMcpServer {
    pub server: crate::mcp::registry::McpServer,
    pub score: f64,
}

/// Queue for managing missing capability resolution requests
#[derive(Debug)]
pub struct MissingCapabilityQueue {
    /// Pending resolution requests
    queue: VecDeque<MissingCapabilityRequest>,
    /// Capabilities currently being resolved (to avoid duplicates)
    in_progress: HashSet<String>,
    /// Failed resolutions with retry information
    failed_resolutions: HashMap<String, (MissingCapabilityRequest, std::time::SystemTime)>,
    /// Maximum number of resolution attempts per capability
    max_attempts: u32,
}

impl MissingCapabilityQueue {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            in_progress: HashSet::new(),
            failed_resolutions: HashMap::new(),
            max_attempts: 3,
        }
    }

    /// Add a missing capability request to the resolution queue
    pub fn enqueue(&mut self, request: MissingCapabilityRequest) {
        // Don't add if already in progress or permanently failed
        if self.in_progress.contains(&request.capability_id) {
            return;
        }

        if self.failed_resolutions.contains_key(&request.capability_id) {
            return;
        }

        // Check if already in queue
        if self
            .queue
            .iter()
            .any(|r| r.capability_id == request.capability_id)
        {
            return;
        }

        self.queue.push_back(request);
    }

    /// Get the next resolution request from the queue
    pub fn dequeue(&mut self) -> Option<MissingCapabilityRequest> {
        if let Some(request) = self.queue.pop_front() {
            self.in_progress.insert(request.capability_id.clone());
            Some(request)
        } else {
            None
        }
    }

    /// Mark a capability as completed (resolved or permanently failed)
    pub fn mark_completed(&mut self, capability_id: &str, result: ResolutionResult) {
        self.in_progress.remove(capability_id);

        match result {
            ResolutionResult::PermanentlyFailed { .. } => {
                // Add to failed_resolutions to prevent future attempts
                if let Some(request) = self.queue.iter().find(|r| r.capability_id == capability_id)
                {
                    self.failed_resolutions.insert(
                        capability_id.to_string(),
                        (request.clone(), std::time::SystemTime::now()),
                    );
                }
            }
            ResolutionResult::Failed {
                retry_after: _retry_after,
                ..
            } => {
                // Remove from in_progress, will be retried later
                self.in_progress.remove(capability_id);
                // If retry_after is specified, we could implement a delayed retry mechanism
            }
            ResolutionResult::Resolved { .. } => {
                // Successfully resolved, remove from failed_resolutions if present
                self.failed_resolutions.remove(capability_id);
            }
        }
    }

    /// Check if there are pending resolution requests
    pub fn has_pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Get queue statistics
    pub fn stats(&self) -> QueueStats {
        QueueStats {
            pending_count: self.queue.len(),
            in_progress_count: self.in_progress.len(),
            failed_count: self.failed_resolutions.len(),
        }
    }
}

/// Statistics about the resolution queue
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub pending_count: usize,
    pub in_progress_count: usize,
    pub failed_count: usize,
}

/// Resolution attempt record for tracking retry history
#[derive(Debug, Clone)]
pub struct ResolutionAttempt {
    /// Capability ID being resolved
    pub capability_id: String,
    /// Timestamp of the attempt
    pub attempted_at: std::time::SystemTime,
    /// Number of attempts so far
    pub attempt_count: u32,
    /// Strategy name used
    pub strategy_name: String,
    /// Success status
    pub success: bool,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Next retry time (if failed)
    pub next_retry_at: Option<std::time::SystemTime>,
}

/// Main resolver that coordinates missing capability resolution
pub struct MissingCapabilityResolver {
    /// The resolution queue
    queue: Arc<Mutex<MissingCapabilityQueue>>,
    /// Reference to the capability marketplace for discovery and registration
    marketplace: Arc<CapabilityMarketplace>,
    /// Reference to the checkpoint archive for auto-resume functionality
    checkpoint_archive: Arc<CheckpointArchive>,
    /// Configuration for resolution behavior
    config: ResolverConfig,
    /// Feature flag checker for controlling system behavior
    feature_checker: FeatureFlagChecker,
    /// Server trust registry for managing server trust and user interaction
    trust_registry: ServerTrustRegistry,
    /// Optional delegating arbiter for LLM-based synthesis
    delegating_arbiter: Arc<RwLock<Option<Arc<DelegatingArbiter>>>>,
    /// Persistent alias store for previously selected tools
    alias_store: Arc<RwLock<ToolAliasStore>>,
    /// Optional observer for structured resolution events
    event_observer: Arc<RwLock<Option<Arc<dyn ResolutionObserver>>>>,
    /// Resolution history for backoff tracking
    resolution_history: Arc<std::sync::RwLock<HashMap<String, Vec<ResolutionAttempt>>>>,
}

/// Configuration for the missing capability resolver
#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// Maximum number of resolution attempts per capability
    pub max_attempts: u32,
    /// Whether to enable automatic resolution attempts
    pub auto_resolve: bool,
    /// Whether to log detailed resolution information
    pub verbose_logging: bool,
    /// Base backoff delay in seconds for retry
    pub base_backoff_seconds: u64,
    /// Maximum backoff delay in seconds
    pub max_backoff_seconds: u64,
    /// Whether to enable high-risk auto-resolution (bypasses human approval)
    pub high_risk_auto_resolution: bool,
    /// Human approval timeout in hours
    pub human_approval_timeout_hours: u64,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            auto_resolve: true,
            verbose_logging: false,
            base_backoff_seconds: 60,
            max_backoff_seconds: 3600,
            high_risk_auto_resolution: false,
            human_approval_timeout_hours: 24,
        }
    }
}

/// Risk priority level for capability resolution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionPriority {
    /// Low risk - can be auto-resolved
    Low,
    /// Medium risk - auto-resolve with monitoring
    Medium,
    /// High risk - requires human approval
    High,
    /// Critical risk - manual intervention required
    Critical,
}

/// Risk assessment for a capability resolution
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    /// Overall risk level
    pub priority: ResolutionPriority,
    /// Risk factors identified
    pub risk_factors: Vec<String>,
    /// Security concerns
    pub security_concerns: Vec<String>,
    /// Compliance requirements
    pub compliance_requirements: Vec<String>,
    /// Human approval required
    pub requires_human_approval: bool,
}

impl RiskAssessment {
    /// Assess risk for a capability based on its ID and context
    pub fn assess(capability_id: &str, config: &ResolverConfig) -> Self {
        let mut risk_factors = Vec::new();
        let mut security_concerns = Vec::new();
        let mut compliance_requirements = Vec::new();

        // Analyze capability ID for risk indicators
        let id_lower = capability_id.to_lowercase();
        
        if id_lower.contains("admin") || id_lower.contains("root") || id_lower.contains("sudo") {
            risk_factors.push("Administrative capability detected".to_string());
            security_concerns.push("High privilege access required".to_string());
        }

        if id_lower.contains("payment") || id_lower.contains("financial") || id_lower.contains("billing") {
            risk_factors.push("Financial capability detected".to_string());
            compliance_requirements.push("PCI-DSS compliance required".to_string());
        }

        if id_lower.contains("auth") || id_lower.contains("security") || id_lower.contains("credential") {
            risk_factors.push("Security-related capability".to_string());
            security_concerns.push("Authentication/authorization access".to_string());
        }

        if id_lower.contains("database") || id_lower.contains("storage") || id_lower.contains("delete") {
            risk_factors.push("Data access capability".to_string());
            compliance_requirements.push("Data protection compliance required".to_string());
        }

        if id_lower.contains("pii") || id_lower.contains("personal") || id_lower.contains("gdpr") {
            risk_factors.push("Personal data handling".to_string());
            compliance_requirements.push("GDPR compliance required".to_string());
        }

        // Determine priority based on risk factors
        let priority = if security_concerns.len() > 1 || compliance_requirements.len() > 1 {
            ResolutionPriority::Critical
        } else if !security_concerns.is_empty() || !compliance_requirements.is_empty() {
            ResolutionPriority::High
        } else if !risk_factors.is_empty() {
            ResolutionPriority::Medium
        } else {
            ResolutionPriority::Low
        };

        let requires_human_approval = priority == ResolutionPriority::Critical
            || (priority == ResolutionPriority::High && !config.high_risk_auto_resolution);

        Self {
            priority,
            risk_factors,
            security_concerns,
            compliance_requirements,
            requires_human_approval,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResolverStats {
    pub pending_count: usize,
    pub in_progress_count: usize,
    pub resolved_count: usize,
    pub failed_count: usize,
}

impl MissingCapabilityResolver {
    fn build_mcp_auth_headers(&self, server_name: &str) -> Option<HashMap<String, String>> {
        self.get_mcp_auth_token(server_name).map(|token| {
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            headers
        })
    }

    fn get_mcp_auth_token(&self, server_name: &str) -> Option<String> {
        let namespace = if let Some(slash_pos) = server_name.find('/') {
            &server_name[..slash_pos]
        } else {
            server_name
        };

        let normalized_namespace = namespace.replace('-', "_").to_uppercase();
        let server_specific_var = format!("{}_MCP_TOKEN", normalized_namespace);

        if let Ok(token) = std::env::var(&server_specific_var) {
            if !token.is_empty() {
                return Some(token);
            }
        }

        if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }

        None
    }

    fn suggest_mcp_token_env_var(&self, server_name: &str) -> String {
        let namespace = if let Some(slash_pos) = server_name.find('/') {
            &server_name[..slash_pos]
        } else {
            server_name
        };

        let normalized = namespace.replace('-', "_").to_uppercase();
        format!("{}_MCP_TOKEN", normalized)
    }

    fn attach_resolution_metadata(
        &self,
        manifest: &mut CapabilityManifest,
        server_name: &str,
        server_url: &str,
        strategy: &str,
        input_remap: Option<&HashMap<String, String>>,
    ) {
        manifest
            .metadata
            .insert("mcp_server".to_string(), server_name.to_string());
        manifest
            .metadata
            .insert("mcp_server_url".to_string(), server_url.to_string());
        manifest
            .metadata
            .insert("resolution_strategy".to_string(), strategy.to_string());
        if let Some(remap) = input_remap {
            if !remap.is_empty() {
                if let Ok(json) = serde_json::to_string(remap) {
                    manifest
                        .metadata
                        .insert("mcp_input_remap".to_string(), json);
                }
            }
        }
    }

    fn build_need_from_request(
        &self,
        capability_id: &str,
        request: &MissingCapabilityRequest,
    ) -> CapabilityNeed {
        let mut rationale = request
            .context
            .get("step_description")
            .cloned()
            .or_else(|| request.context.get("step_name").cloned())
            .unwrap_or_else(|| format!("Resolve missing capability {}", capability_id));

        if rationale.is_empty() {
            rationale = format!("Resolve missing capability {}", capability_id);
        }

        if !request.arguments.is_empty() {
            let samples: Vec<String> = request.arguments.iter().map(sanitize_value).collect();
            rationale.push_str("\nExample arguments: ");
            rationale.push_str(&samples.join(", "));
        }

        let required_inputs = request
            .context
            .get("required_inputs")
            .map(|value| Self::split_list_field(value))
            .unwrap_or_default();
        let expected_outputs = request
            .context
            .get("expected_outputs")
            .map(|value| Self::split_list_field(value))
            .unwrap_or_default();

        CapabilityNeed::new(
            capability_id.to_string(),
            required_inputs,
            expected_outputs,
            rationale,
        )
    }

    fn split_list_field(raw: &str) -> Vec<String> {
        raw.split(',')
            .map(|entry| entry.trim())
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.to_string())
            .collect()
    }

    fn compute_tool_score(
        &self,
        capability_id: &str,
        tool_name: &str,
        description: &str,
        need: &CapabilityNeed,
    ) -> f64 {
        let mut score =
            calculate_description_match_score(need.rationale.as_str(), description, tool_name);

        let overlap = Self::keyword_overlap(capability_id, tool_name);
        score += overlap * 2.5;

        let capability_last = capability_id
            .split('.')
            .last()
            .unwrap_or(capability_id)
            .to_ascii_lowercase();
        let tool_lower = tool_name.to_ascii_lowercase();

        if tool_lower == capability_last {
            score += 2.0;
        } else if tool_lower.contains(&capability_last) {
            score += 1.0;
        }

        if capability_id
            .replace('.', "_")
            .to_ascii_lowercase()
            .contains(&tool_lower)
        {
            score += 1.0;
        }

        // Use discovery module's action verb matching for synonym handling
        // This handles synonyms like get/list/fetch/retrieve, create/add, etc.
        let capability_verbs = extract_action_verbs(&capability_last);
        let tool_verbs = extract_action_verbs(&tool_lower);
        let action_verb_score = calculate_action_verb_match_score(&capability_verbs, &tool_verbs);

        // Boost score if action verbs match well (synonyms count as 0.8, exact as 1.0)
        if action_verb_score > 0.0 {
            // Scale the action verb score to add meaningful boost
            // Exact match (1.0) -> +2.0, synonym match (0.8) -> +1.5, no match (0.0) -> +0.0
            score += action_verb_score * 2.0;
        }

        score
    }

    fn tokenize_identifier(identifier: &str) -> HashSet<String> {
        identifier
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|token| !token.is_empty())
            .map(|token| token.to_ascii_lowercase())
            .collect()
    }

    fn keyword_overlap(lhs: &str, rhs: &str) -> f64 {
        let lhs_tokens = Self::tokenize_identifier(lhs);
        let rhs_tokens = Self::tokenize_identifier(rhs);

        if lhs_tokens.is_empty() || rhs_tokens.is_empty() {
            return 0.0;
        }

        let intersection = lhs_tokens.intersection(&rhs_tokens).count();
        if intersection == 0 {
            return 0.0;
        }

        intersection as f64 / lhs_tokens.len().max(rhs_tokens.len()) as f64
    }

    fn extract_input_keys_from_type(schema: Option<&TypeExpr>) -> Vec<String> {
        let mut keys = HashSet::new();
        if let Some(expr) = schema {
            Self::collect_map_keys(expr, &mut keys);
        }
        let mut list: Vec<String> = keys.into_iter().collect();
        list.sort();
        list
    }

    fn collect_map_keys(expr: &TypeExpr, keys: &mut HashSet<String>) {
        match expr {
            TypeExpr::Map { entries, wildcard } => {
                for entry in entries {
                    keys.insert(value_conversion::map_key_to_string(&rtfs::ast::MapKey::Keyword(entry.key.clone())));
                    Self::collect_map_keys(&entry.value_type, keys);
                }
                if let Some(wild) = wildcard {
                    Self::collect_map_keys(wild, keys);
                }
            }
            TypeExpr::Optional(inner) => Self::collect_map_keys(inner, keys),
            TypeExpr::Union(options) => {
                for option in options {
                    Self::collect_map_keys(option, keys);
                }
            }
            TypeExpr::Vector(inner) => Self::collect_map_keys(inner, keys),
            TypeExpr::Array { element_type, .. } => Self::collect_map_keys(element_type, keys),
            TypeExpr::Tuple(entries) => {
                for entry in entries {
                    Self::collect_map_keys(entry, keys);
                }
            }
            _ => {}
        }
    }

    async fn run_tool_selector(
        &self,
        capability_id: &str,
        need: &CapabilityNeed,
        candidates: &[ToolPromptCandidate],
    ) -> RuntimeResult<Option<ToolSelectionResult>> {
        if !self.feature_checker.is_tool_selector_enabled() || candidates.is_empty() {
            return Ok(None);
        }

        let delegating = {
            let guard = self.delegating_arbiter.read().unwrap();
            guard.clone()
        };
        let Some(delegating) = delegating else {
            return Ok(None);
        };

        let config = self.feature_checker.tool_selection_config().clone();
        let mut limited: Vec<ToolPromptCandidate> = candidates
            .iter()
            .take(config.max_tools.min(candidates.len()))
            .cloned()
            .collect();

        let need_block = Self::render_need_block(capability_id, need);
        let tools_block = Self::render_tools_block(&mut limited, config.max_description_chars);

        let mut vars = HashMap::new();
        vars.insert("need_block".to_string(), need_block);
        vars.insert("tools_block".to_string(), tools_block);

        let prompt = CAPABILITY_PROMPT_MANAGER
            .render(&config.prompt_id, &config.prompt_version, &vars)
            .map_err(|e| {
                RuntimeError::Generic(format!("Failed to render tool selection prompt: {}", e))
            })?;

        self.emit_event(
            capability_id,
            "tool_selector",
            format!(
                "Invoking LLM tool selector with {} candidates",
                limited.len()
            ),
            if should_log_debug_prompts() {
                Some(Self::truncate_text(&prompt, 600))
            } else {
                None
            },
        );

        if should_log_debug_prompts() {
            eprintln!("┌─ Tool Selector Prompt ─────────────────────────────────────");
            eprintln!("{}", prompt);
            eprintln!("└────────────────────────────────────────────────────────────");
        }

        let response = delegating
            .generate_raw_text(&prompt)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Tool selector request failed: {}", e)))?;

        if should_log_debug_prompts() {
            eprintln!("┌─ Tool Selector Response ───────────────────────────────────");
            eprintln!("{}", response.trim());
            eprintln!("└────────────────────────────────────────────────────────────");
        }

        let rtfs = extract_rtfs_block(&response);
        let expr = parse_expression(&rtfs).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to parse tool selector response as RTFS: {:?}",
                e
            ))
        })?;

        self.parse_tool_selector_response(expr)
    }

    fn render_need_block(capability_id: &str, need: &CapabilityNeed) -> String {
        let mut block = format!(
            "- capability-class: {}\n- rationale: {}\n",
            capability_id,
            need.rationale.trim()
        );
        let req = if need.required_inputs.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", need.required_inputs.join(", "))
        };
        let outputs = if need.expected_outputs.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", need.expected_outputs.join(", "))
        };
        block.push_str(&format!("- required-inputs: {}\n", req));
        block.push_str(&format!("- expected-outputs: {}\n", outputs));
        block
    }

    fn render_tools_block(
        candidates: &mut [ToolPromptCandidate],
        max_description_chars: usize,
    ) -> String {
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
            .iter()
            .enumerate()
            .map(|(idx, candidate)| {
                let desc = Self::truncate_text(candidate.description.trim(), max_description_chars);
                let inputs = if candidate.input_keys.is_empty() {
                    "(none)".to_string()
                } else {
                    candidate.input_keys.join(", ")
                };
                format!(
                    "{:02}. {}\n    description: {}\n    input-keys: {}\n    heuristic-score: {:.2}",
                    idx + 1,
                    candidate.tool_name,
                    desc,
                    inputs,
                    candidate.score
                )
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    fn truncate_text(text: &str, max_len: usize) -> String {
        if text.len() <= max_len {
            text.to_string()
        } else {
            let mut truncated = text[..max_len].to_string();
            truncated.push_str("…");
            truncated
        }
    }

    fn parse_tool_selector_response(
        &self,
        expr: Expression,
    ) -> RuntimeResult<Option<ToolSelectionResult>> {
        match expr {
            Expression::Literal(Literal::Nil) => Ok(None),
            Expression::Map(entries) => {
                let mut map = HashMap::new();
                for (key, value) in entries {
                    let key_str = value_conversion::map_key_to_string(&key);
                    map.insert(key_str, value);
                }

                let tool_name = map
                    .remove("tool_name")
                    .or_else(|| map.remove(":tool_name"))
                    .and_then(|expr| Self::literal_to_string(&expr))
                    .ok_or_else(|| {
                        RuntimeError::Generic(
                            "Tool selector response missing :tool_name field".to_string(),
                        )
                    })?;

                let input_remap_expr = map
                    .remove("input_remap")
                    .or_else(|| map.remove(":input_remap"))
                    .unwrap_or(Expression::Literal(Literal::Nil));

                let input_remap = Self::expression_to_string_map(&input_remap_expr)?;

                Ok(Some(ToolSelectionResult {
                    tool_name,
                    input_remap,
                }))
            }
            other => Err(RuntimeError::Generic(format!(
                "Tool selector response must be a map, got {:?}",
                other
            ))),
        }
    }

    fn literal_to_string(expr: &Expression) -> Option<String> {
        match expr {
            Expression::Literal(Literal::String(s)) => Some(s.clone()),
            Expression::Literal(Literal::Keyword(k)) => {
                Some(value_conversion::map_key_to_string(&rtfs::ast::MapKey::Keyword(k.clone())))
            }
            Expression::Literal(Literal::Symbol(symbol)) => Some(symbol.0.clone()),
            Expression::Literal(Literal::Nil) => None,
            _ => None,
        }
    }

    fn expression_to_string_map(expr: &Expression) -> RuntimeResult<HashMap<String, String>> {
        match expr {
            Expression::Literal(Literal::Nil) => Ok(HashMap::new()),
            Expression::Map(entries) => {
                let mut map = HashMap::new();
                for (key, value) in entries {
                    let key_str = value_conversion::map_key_to_string(&key);
                    if let Some(val) = Self::literal_to_string(value) {
                        map.insert(key_str, val);
                    }
                }
                Ok(map)
            }
            other => Err(RuntimeError::Generic(format!(
                "Expected map for :input_remap, got {:?}",
                other
            ))),
        }
    }

    /// Normalize capability IDs by trimming whitespace and stray quotes
    fn normalize_capability_id(input: &str) -> String {
        // First trim surrounding whitespace, then strip surrounding quotes,
        // then trim again to remove any whitespace that was inside quotes.
        input.trim().trim_matches('"').trim().to_string()
    }

    /// Create a new missing capability resolver
    pub fn new(
        marketplace: Arc<CapabilityMarketplace>,
        checkpoint_archive: Arc<CheckpointArchive>,
        config: ResolverConfig,
        feature_config: MissingCapabilityConfig,
    ) -> Self {
        Self {
            queue: Arc::new(Mutex::new(MissingCapabilityQueue::new())),
            marketplace,
            checkpoint_archive,
            config,
            feature_checker: FeatureFlagChecker::new(feature_config),
            trust_registry: create_default_trust_registry(),
            delegating_arbiter: Arc::new(RwLock::new(None)),
            alias_store: Arc::new(RwLock::new(ToolAliasStore::load_default())),
            event_observer: Arc::new(RwLock::new(None)),
            resolution_history: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Create a new missing capability resolver with custom trust registry
    pub fn with_trust_registry(
        marketplace: Arc<CapabilityMarketplace>,
        checkpoint_archive: Arc<CheckpointArchive>,
        config: ResolverConfig,
        feature_config: MissingCapabilityConfig,
        trust_registry: ServerTrustRegistry,
    ) -> Self {
        Self {
            queue: Arc::new(Mutex::new(MissingCapabilityQueue::new())),
            marketplace,
            checkpoint_archive,
            config,
            feature_checker: FeatureFlagChecker::new(feature_config),
            trust_registry,
            delegating_arbiter: Arc::new(RwLock::new(None)),
            alias_store: Arc::new(RwLock::new(ToolAliasStore::load_default())),
            event_observer: Arc::new(RwLock::new(None)),
            resolution_history: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Inject the delegating arbiter for LLM-backed synthesis.
    pub fn set_delegating_arbiter(&self, arbiter: Option<Arc<DelegatingArbiter>>) {
        if let Ok(mut slot) = self.delegating_arbiter.write() {
            *slot = arbiter;
        }
    }

    /// Attach an observer to receive structured resolution events.
    pub fn set_event_observer(&self, observer: Option<Arc<dyn ResolutionObserver>>) {
        if let Ok(mut slot) = self.event_observer.write() {
            *slot = observer;
        }
    }

    fn emit_event(
        &self,
        capability_id: &str,
        stage: &'static str,
        summary: impl Into<String>,
        detail: Option<String>,
    ) {
        if let Ok(slot) = self.event_observer.read() {
            if let Some(observer) = slot.as_ref() {
                observer.on_event(ResolutionEvent {
                    capability_id: capability_id.to_string(),
                    stage,
                    summary: summary.into(),
                    detail,
                });
            }
        }
    }

    fn lookup_alias(&self, capability_id: &str) -> Option<ToolAliasRecord> {
        self.alias_store
            .read()
            .ok()
            .and_then(|store| store.lookup(capability_id))
    }

    fn persist_alias(&self, record: ToolAliasRecord) {
        if let Ok(mut store) = self.alias_store.write() {
            if let Err(err) = store.insert(record) {
                eprintln!("⚠️  Failed to persist tool alias: {}", err);
            }
        }
    }

    fn remove_alias(&self, capability_pattern: &str) {
        if let Ok(mut store) = self.alias_store.write() {
            if let Err(err) = store.remove(capability_pattern) {
                eprintln!(
                    "⚠️  Failed to remove tool alias for '{}': {}",
                    capability_pattern, err
                );
            }
        }
    }

    async fn try_resolve_with_alias(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        let Some(alias) = self.lookup_alias(capability_id) else {
            return Ok(None);
        };

        if self.config.verbose_logging {
            eprintln!(
                "🔁 Alias cache hit for '{}' → {} / {}",
                capability_id, alias.server_name, alias.tool_name
            );
        }

        match ToolAliasStore::materialize_alias(&alias).await {
            Ok(Some(manifest)) => {
                if let Err(err) = self.persist_discovered_mcp_capability(&manifest) {
                    eprintln!(
                        "⚠️  Failed to persist MCP alias capability '{}': {}",
                        manifest.id, err
                    );
                }
                return Ok(Some(manifest));
            }
            Ok(None) => {
                if self.config.verbose_logging {
                    eprintln!(
                        "⚠️  Alias for '{}' is stale (tool not found). Removing from cache.",
                        capability_id
                    );
                }
                self.remove_alias(&alias.capability_pattern);
                Ok(None)
            }
            Err(err) => {
                eprintln!(
                    "⚠️  Failed to materialize alias for '{}': {}",
                    capability_id, err
                );
                Ok(None)
            }
        }
    }

    /// Handle a missing capability error by adding it to the resolution queue
    pub fn handle_missing_capability(
        &self,
        capability_id: String,
        arguments: Vec<rtfs::runtime::values::Value>,
        context: HashMap<String, String>,
    ) -> RuntimeResult<()> {
        let normalized_id = Self::normalize_capability_id(&capability_id);
        eprintln!(
            "🔍 HANDLE MISSING: Attempting to handle missing capability '{}'",
            normalized_id
        );

        // Check if missing capability resolution is enabled
        if !self.feature_checker.is_enabled() {
            eprintln!("❌ HANDLE MISSING: Feature is disabled");
            return Ok(()); // Silently ignore if disabled
        }

        if !self.feature_checker.is_runtime_detection_enabled() {
            eprintln!("❌ HANDLE MISSING: Runtime detection is disabled");
            return Ok(()); // Silently ignore if runtime detection is disabled
        }

        eprintln!("✅ HANDLE MISSING: Feature is enabled, proceeding with queue");
        let request = MissingCapabilityRequest {
            capability_id: normalized_id.clone(),
            arguments,
            context,
            requested_at: std::time::SystemTime::now(),
            attempt_count: 0,
        };

        {
            let mut queue = self.queue.lock().unwrap();
            queue.enqueue(request);
        }

        if self.config.verbose_logging {
            eprintln!(
                "🔍 MISSING CAPABILITY: Added '{}' to resolution queue",
                normalized_id
            );
        }

        // Emit audit event for missing capability
        self.emit_missing_capability_audit(&normalized_id)?;

        Ok(())
    }

    /// Attempt to resolve a missing capability using various discovery methods
    pub async fn resolve_capability(
        &self,
        request: &MissingCapabilityRequest,
    ) -> RuntimeResult<ResolutionResult> {
        let capability_id = &request.capability_id;
        let capability_id_normalized = Self::normalize_capability_id(capability_id);

        quiet_eprintln!(
            "🔍 RESOLVING: Attempting to resolve capability '{}'",
            capability_id_normalized
        );

        self.emit_event(
            &capability_id_normalized,
            "start",
            format!(
                "Resolution attempt for '{}' (attempt #{})",
                capability_id_normalized,
                request.attempt_count + 1
            ),
            None,
        );

        // Phase 1: Cheap race-condition check against marketplace
        // Always perform this, even when auto-resolution is disabled.
        {
            let capabilities = self.marketplace.capabilities.read().await;
            quiet_eprintln!(
                "🔍 DEBUG: Checking marketplace for '{}' - found {} capabilities",
                capability_id_normalized,
                capabilities.len()
            );
            if capabilities.contains_key(&capability_id_normalized) {
                self.emit_event(
                    &capability_id_normalized,
                    "marketplace",
                    "Capability already registered in marketplace",
                    None,
                );
                return Ok(ResolutionResult::Resolved {
                    capability_id: capability_id_normalized.clone(),
                    resolution_method: "marketplace_found".to_string(),
                    provider_info: Some("already_registered".to_string()),
                });
            }
            quiet_eprintln!(
                "✅ Capability '{}' is missing from marketplace - proceeding with discovery",
                capability_id_normalized
            );
        }

        // Phase 2: Fan-out discovery requires auto-resolution to be enabled
        if !self.feature_checker.is_auto_resolution_enabled() {
            return Ok(ResolutionResult::Failed {
                capability_id: capability_id_normalized.clone(),
                reason: "Auto-resolution is disabled".to_string(),
                retry_after: None,
            });
        }

        self.emit_event(
            &capability_id_normalized,
            "alias_lookup",
            "Checking cached tool aliases",
            None,
        );

        if let Some(alias_manifest) = self
            .try_resolve_with_alias(&capability_id_normalized)
            .await?
        {
            self.marketplace
                .register_capability_manifest(alias_manifest.clone())
                .await?;

            self.emit_event(
                &capability_id_normalized,
                "alias_lookup",
                format!("Resolved via stored alias: {}", alias_manifest.id),
                alias_manifest
                    .metadata
                    .get("mcp_tool_name")
                    .cloned()
                    .map(|tool| format!("tool={}", tool)),
            );

            self.trigger_auto_resume_for_capability(&capability_id_normalized)
                .await?;

            return Ok(ResolutionResult::Resolved {
                capability_id: alias_manifest.id.clone(),
                resolution_method: "alias_cache".to_string(),
                provider_info: Some(format!("{:?}", alias_manifest.provider)),
            });
        }

        // Try to find similar capabilities using marketplace discovery
        // Always log discovery start (not suppressed) so user knows discovery is happening
        eprintln!(
            "🔍 DISCOVERY: Starting discovery for capability '{}'",
            capability_id_normalized
        );
        let discovery_result = self
            .discover_capability(&capability_id_normalized, request)
            .await?;
        if discovery_result.is_some() {
            eprintln!(
                "✅ DISCOVERY: Successfully discovered capability '{}'",
                capability_id_normalized
            );
        } else {
            eprintln!(
                "❌ DISCOVERY: No capability found for '{}' after discovery attempts",
                capability_id_normalized
            );
        }

        match discovery_result {
            Some(manifest) => {
                let actual_capability_id = manifest.id.clone();
                eprintln!(
                    "✅ DISCOVERY: Successfully discovered capability '{}' -> registered as '{}'",
                    capability_id_normalized, actual_capability_id
                );
                quiet_eprintln!(
                    "✅ DISCOVERY: Successfully discovered capability '{}'",
                    capability_id_normalized
                );
                // Register the discovered capability under its actual ID
                self.marketplace
                    .register_capability_manifest(manifest.clone())
                    .await?;

                // Trigger auto-resume for both the requested ID and the actual ID
                self.trigger_auto_resume_for_capability(&capability_id_normalized)
                    .await?;
                if actual_capability_id != capability_id_normalized {
                    self.trigger_auto_resume_for_capability(&actual_capability_id)
                        .await?;
                }

                self.emit_event(
                    &capability_id_normalized,
                    "result",
                    "Capability resolved via marketplace discovery",
                    manifest.metadata.get("mcp_tool_name").cloned(),
                );

                // Return the ACTUAL manifest ID, not the requested ID
                // This ensures the lookup in the example code will find it
                Ok(ResolutionResult::Resolved {
                    capability_id: actual_capability_id.clone(),
                    resolution_method: "marketplace_discovery".to_string(),
                    provider_info: Some(format!("{:?}", manifest.provider)),
                })
            }
            None => {
                // Try pure RTFS generation as a cheap, local fallback before invoking LLM synthesis.
                if let Some(manifest) = self
                    .attempt_pure_rtfs_generation(request, &capability_id_normalized)
                    .await?
                {
                    self.marketplace
                        .register_capability_manifest(manifest.clone())
                        .await?;

                    self.trigger_auto_resume_for_capability(&capability_id_normalized)
                        .await?;

                    return Ok(ResolutionResult::Resolved {
                        capability_id: manifest.id.clone(),
                        resolution_method: "pure_rtfs_generation".to_string(),
                        provider_info: Some("pure_rtfs_generated".to_string()),
                    });
                }

                // 2. User Interaction Strategy
                if let Some(manifest) = self
                    .attempt_user_interaction(request, &capability_id_normalized)
                    .await?
                {
                    self.marketplace
                        .register_capability_manifest(manifest.clone())
                        .await?;

                    self.trigger_auto_resume_for_capability(&capability_id_normalized)
                        .await?;

                    return Ok(ResolutionResult::Resolved {
                        capability_id: manifest.id.clone(),
                        resolution_method: "user_interaction".to_string(),
                        provider_info: Some("user_provided".to_string()),
                    });
                }

                // 3. Service Discovery Hint Strategy
                if let Some(manifest) = self
                    .attempt_service_discovery_hint(request, &capability_id_normalized)
                    .await?
                {
                    self.marketplace
                        .register_capability_manifest(manifest.clone())
                        .await?;

                    self.trigger_auto_resume_for_capability(&capability_id_normalized)
                        .await?;

                    return Ok(ResolutionResult::Resolved {
                        capability_id: manifest.id.clone(),
                        resolution_method: "service_discovery_hint".to_string(),
                        provider_info: Some("user_hint".to_string()),
                    });
                }

                // 4. External LLM Hint Strategy
                if self.feature_checker.is_llm_synthesis_enabled() {
                    if let Some(manifest) = self
                        .attempt_llm_capability_synthesis(request, &capability_id_normalized)
                        .await?
                    {
                        self.marketplace
                            .register_capability_manifest(manifest.clone())
                            .await?;

                        self.trigger_auto_resume_for_capability(&capability_id_normalized)
                            .await?;

                        return Ok(ResolutionResult::Resolved {
                            capability_id: manifest.id.clone(),
                            resolution_method: "llm_synthesis".to_string(),
                            provider_info: Some("llm_auto_generated".to_string()),
                        });
                    }
                }

                // No capability found through discovery or synthesis
                self.emit_event(
                    &capability_id_normalized,
                    "result",
                    "Resolution failed after discovery and synthesis attempts",
                    None,
                );
                Ok(ResolutionResult::Failed {
                    capability_id: capability_id_normalized.clone(),
                    reason: "No matching capability found through discovery".to_string(),
                    retry_after: Some(std::time::Duration::from_secs(60)), // Retry in 1 minute
                })
            }
        }
    }

    async fn attempt_llm_capability_synthesis(
        &self,
        request: &MissingCapabilityRequest,
        capability_id_normalized: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        let delegating = {
            let guard = self.delegating_arbiter.read().unwrap();
            guard.clone()
        };

        let Some(delegating) = delegating else {
            if self.config.verbose_logging {
                eprintln!(
                    "ℹ️  LLM synthesis skipped for '{}' (no delegating arbiter configured)",
                    capability_id_normalized
                );
            }
            return Ok(None);
        };

        let prompt = self
            .build_capability_prompt(request, capability_id_normalized)
            .await?;

        if should_log_debug_prompts() {
            eprintln!("┌─ Capability Prompt ────────────────────────────────────────");
            eprintln!("{}", prompt);
            eprintln!("└────────────────────────────────────────────────────────────");
        }

        let response = delegating
            .generate_raw_text(&prompt)
            .await
            .map_err(|e| RuntimeError::Generic(format!("LLM synthesis request failed: {}", e)))?;

        if should_log_debug_prompts() {
            eprintln!("┌─ LLM Capability Response ──────────────────────────────────");
            eprintln!("{}", response.trim());
            eprintln!("└────────────────────────────────────────────────────────────");
        }

        let Some(capability_rtfs) = extract_capability_rtfs_from_response(&response) else {
            if self.config.verbose_logging {
                eprintln!("❌ LLM synthesis response did not contain a `(capability ...)` form.");
            }
            return Ok(None);
        };

        match self
            .manifest_from_rtfs(&capability_rtfs, capability_id_normalized)
            .await
        {
            Ok(mut manifest) => {
                if let Ok(storage_path) = self.persist_llm_generated_capability(&manifest).await {
                    manifest.metadata.insert(
                        "storage_path".to_string(),
                        storage_path.display().to_string(),
                    );
                }

                self.emit_event(
                    capability_id_normalized,
                    "llm_synthesis",
                    "LLM produced a candidate capability",
                    Some(Self::truncate_text(&capability_rtfs, 400)),
                );

                Ok(Some(manifest))
            }
            Err(err) => {
                if self.config.verbose_logging {
                    eprintln!("❌ Failed to parse LLM capability: {}", err);
                }
                Ok(None)
            }
        }
    }

    async fn persist_llm_generated_capability(
        &self,
        manifest: &CapabilityManifest,
    ) -> RuntimeResult<PathBuf> {
        let implementation_code = manifest
            .metadata
            .get("rtfs_implementation")
            .cloned()
            .ok_or_else(|| {
                RuntimeError::Generic(
                    "LLM-generated capability missing rtfs implementation metadata".to_string(),
                )
            })?;

        let storage_root = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./capabilities"));
        let storage_root = storage_root.join("generated");
        std::fs::create_dir_all(&storage_root).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create storage directory: {}", e))
        })?;

        let capability_dir = storage_root.join(Self::sanitize_capability_dir_name(&manifest.id));
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to create capability directory '{}': {}",
                capability_dir.display(),
                e
            ))
        })?;

        let rtfs_source = Self::manifest_to_rtfs(manifest, &implementation_code);

        let rtfs_path = capability_dir.join("capability.rtfs");
        std::fs::write(&rtfs_path, rtfs_source).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write capability file '{}': {}",
                rtfs_path.display(),
                e
            ))
        })?;

        Ok(rtfs_path)
    }

    fn persist_discovered_mcp_capability(
        &self,
        manifest: &CapabilityManifest,
    ) -> RuntimeResult<Option<PathBuf>> {
        if manifest.metadata.get("mcp_tool_name").is_none() {
            return Ok(None);
        }

        let implementation_code = manifest
            .metadata
            .get("rtfs_implementation")
            .cloned()
            .or_else(|| {
                let server_url = manifest
                    .metadata
                    .get("mcp_server_url")
                    .cloned()
                    .unwrap_or_default();
                let tool_name = manifest
                    .metadata
                    .get("mcp_tool_name")
                    .cloned()
                    .unwrap_or_else(|| manifest.id.clone());
                Some(format!(
                    "(fn [input]\n  ;; MCP Tool: {}\n  (call :ccos.capabilities.mcp.call\n    :server-url \"{}\"\n    :tool-name \"{}\"\n    :input input))",
                    manifest.name, server_url, tool_name
                ))
            })
            .ok_or_else(|| {
                RuntimeError::Generic(
                    format!("MCP capability '{}' missing implementation metadata", manifest.id),
                )
            })?;

        let storage_root = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./capabilities"));
        let storage_root = storage_root.join("discovered").join("mcp");
        std::fs::create_dir_all(&storage_root).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create MCP discovery directory: {}", e))
        })?;

        let id_parts: Vec<&str> = manifest.id.split('.').collect();
        let namespace = if id_parts.len() >= 2 {
            id_parts[0]
        } else {
            "misc"
        };
        let capability_dir = storage_root.join(namespace);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to create capability directory '{}': {}",
                capability_dir.display(),
                e
            ))
        })?;

        let file_stem = if id_parts.len() >= 2 {
            id_parts[1..].join("_")
        } else {
            Self::sanitize_capability_dir_name(&manifest.id)
        };
        let file_path = capability_dir.join(format!("{}.rtfs", file_stem));

        let mut manifest_clone = manifest.clone();
        manifest_clone.metadata.insert(
            "rtfs_implementation".to_string(),
            implementation_code.clone(),
        );
        let rtfs_source = Self::manifest_to_rtfs(&manifest_clone, &implementation_code);

        std::fs::write(&file_path, rtfs_source).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write MCP capability file '{}': {}",
                file_path.display(),
                e
            ))
        })?;

        Ok(Some(file_path))
    }

    fn sanitize_capability_dir_name(id: &str) -> String {
        id.chars()
            .map(|c| if c == '/' || c == '\\' { '_' } else { c })
            .collect()
    }

    pub fn manifest_to_rtfs(manifest: &CapabilityManifest, implementation_code: &str) -> String {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let escaped_name = Self::escape_string(&manifest.name);
        let escaped_description = Self::escape_string(&manifest.description);
        let language = manifest
            .metadata
            .get("language")
            .map(|s| s.as_str())
            .unwrap_or("rtfs20");

        let provider_str = match &manifest.provider {
            ProviderType::MCP(mcp) => Some(format!(
                r#"{{
    :type "mcp"
    :server_endpoint "{}"
    :tool_name "{}"
    :timeout_seconds {}
    :protocol_version "{}"
  }}"#,
                mcp.server_url,
                mcp.tool_name,
                mcp.timeout_ms / 1000,
                manifest
                    .metadata
                    .get("mcp_protocol_version")
                    .map(|s| s.as_str())
                    .unwrap_or("2024-11-05")
            )),
            _ => None,
        };

        let permissions = Self::format_symbol_list(&manifest.permissions);
        let effects = Self::format_symbol_list(&manifest.effects);

        let input_schema = manifest
            .input_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());
        let output_schema = manifest
            .output_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());

        let metadata_block = Self::metadata_to_rtfs(&manifest.metadata);
        let implementation_pretty = match parse_expression(implementation_code) {
            Ok(expr) => expression_to_pretty_rtfs_string(&expr),
            Err(_) => implementation_code.trim().to_string(),
        };

        let implementation_block = Self::indent_block(implementation_pretty.trim_end(), "  ");

        let mut lines = Vec::new();
        lines.push(format!(";; Synthesized capability: {}", manifest.id));
        lines.push(format!(";; Generated: {}", timestamp));
        lines.push("{:type \"capability\"".to_string());
        lines.push(format!(" :id \"{}\"", Self::escape_string(&manifest.id)));
        lines.push(format!(" :name \"{}\"", escaped_name));
        lines.push(format!(" :description \"{}\"", escaped_description));
        lines.push(format!(
            " :version \"{}\"",
            Self::escape_string(&manifest.version)
        ));
        if let Some(provider) = provider_str {
            lines.push("  ;; PROVIDER START".to_string());
            lines.push(format!("  :provider {}", provider));
            lines.push("  ;; PROVIDER END".to_string());
        }
        lines.push(format!(" :language \"{}\"", Self::escape_string(language)));
        lines.push(format!(" :permissions {}", permissions));
        lines.push(format!(" :effects {}", effects));
        lines.push(format!(" :input-schema {}", input_schema));
        lines.push(format!(" :output-schema {}", output_schema));
        if let Some(meta) = metadata_block {
            lines.push(format!(" :metadata {}", meta));
        }
        lines.push(" :implementation".to_string());
        lines.push(implementation_block);
        lines.push("}".to_string());

        lines.join("\n") + "\n"
    }

    fn metadata_to_rtfs(metadata: &HashMap<String, String>) -> Option<String> {
        let mut entries: Vec<String> = metadata
            .iter()
            .filter(|(key, _)| key.as_str() != "rtfs_implementation")
            .map(|(key, value)| {
                format!(
                    ":{key} \"{}\"",
                    Self::escape_string(value),
                    key = key.replace(char::is_whitespace, "_")
                )
            })
            .collect();

        if entries.is_empty() {
            None
        } else {
            entries.sort();
            Some(format!("{{{}}}", entries.join(" ")))
        }
    }

    fn format_symbol_list(symbols: &[String]) -> String {
        if symbols.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", symbols.join(" "))
        }
    }

    fn indent_block(code: &str, prefix: &str) -> String {
        code.lines()
            .map(|line| format!("{}{}", prefix, line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn escape_string(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('"', "\\\"")
    }

    async fn build_capability_prompt(
        &self,
        request: &MissingCapabilityRequest,
        capability_id_normalized: &str,
    ) -> RuntimeResult<String> {
        let args_summary = if request.arguments.is_empty() {
            "No arguments were provided when the capability was invoked.".to_string()
        } else {
            let parts: Vec<String> = request
                .arguments
                .iter()
                .enumerate()
                .map(|(idx, value)| format!("{}: {}", idx, sanitize_value(value)))
                .collect();
            format!(
                "Arguments observed at runtime:\n{}",
                parts
                    .iter()
                    .map(|line| format!("  - {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };

        let context_summary = if request.context.is_empty() {
            "No additional runtime context.".to_string()
        } else {
            let mut pairs: Vec<String> = request
                .context
                .iter()
                .map(|(k, v)| format!("  - {} = {}", k, v))
                .collect();
            pairs.sort();
            format!("Execution context:\n{}", pairs.join("\n"))
        };

        let available_capabilities = self.collect_available_capabilities().await?;

        let mut vars = StdHashMap::new();
        vars.insert(
            "capability_id".to_string(),
            capability_id_normalized.to_string(),
        );
        vars.insert("arguments".to_string(), args_summary);
        vars.insert("runtime_context".to_string(), context_summary);
        vars.insert("available_capabilities".to_string(), available_capabilities);
        vars.insert("prelude_helpers".to_string(), PRELUDE_HELPERS.join("\n"));

        CAPABILITY_PROMPT_MANAGER
            .render(CAPABILITY_PROMPT_ID, CAPABILITY_PROMPT_VERSION, &vars)
            .map_err(|e| RuntimeError::Generic(format!("Failed to render prompt: {}", e)))
    }

    async fn collect_available_capabilities(&self) -> RuntimeResult<String> {
        let caps = self.marketplace.list_capabilities().await;
        if caps.is_empty() {
            return Ok("No local capabilities are currently registered.".to_string());
        }

        let mut lines: Vec<String> = caps
            .iter()
            .filter(|cap| {
                matches!(
                    cap.provider,
                    crate::capability_marketplace::types::ProviderType::Local(_)
                )
            })
            .map(|cap| format!("- {} — {}", cap.id, cap.description))
            .collect();

        if lines.is_empty() {
            lines = caps
                .iter()
                .take(10)
                .map(|cap| format!("- {} — {}", cap.id, cap.description))
                .collect();
        }

        if lines.len() > 12 {
            lines.truncate(12);
        }

        Ok(lines.join("\n"))
    }

    async fn manifest_from_rtfs(
        &self,
        capability_rtfs: &str,
        expected_id: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        let expr = rtfs::parser::parse_expression(capability_rtfs).map_err(|err| {
            RuntimeError::Generic(format!("Failed to parse capability RTFS: {:?}", err))
        })?;

        let normalized = crate::rtfs_bridge::normalizer::normalize_capability_to_map(
            &expr,
            crate::rtfs_bridge::normalizer::NormalizationConfig {
                warn_on_function_call: false,
                validate_after_normalization: true,
            },
        )
        .map_err(|err| {
            RuntimeError::Generic(format!("Capability normalization failed: {}", err))
        })?;

        let Expression::Map(original_map) = normalized else {
            return Err(RuntimeError::Generic(
                "Normalized capability is not a map".to_string(),
            ));
        };

        let mut capability_map: HashMap<MapKey, Expression> = HashMap::new();
        let mut input_schema_expr: Option<Expression> = None;
        let mut output_schema_expr: Option<Expression> = None;
        let mut implementation_expr: Option<Expression> = None;

        for (key, value) in original_map {
            let key_name = match &key {
                MapKey::Keyword(keyword) => keyword.0.as_str(),
                MapKey::String(s) => s.trim_start_matches(':'),
                _ => "",
            };

            match key_name {
                "input-schema" => {
                    input_schema_expr = Some(value.clone());
                    capability_map.insert(key.clone(), value.clone());
                }
                "output-schema" => {
                    output_schema_expr = Some(value.clone());
                    capability_map.insert(key.clone(), value.clone());
                }
                "implementation" => {
                    implementation_expr = Some(value.clone());
                    capability_map.insert(key.clone(), value.clone());
                }
                _ => {
                    capability_map.insert(key.clone(), value.clone());
                }
            }
        }

        let implementation_rtfs = if let Some(expr) = implementation_expr {
            crate::rtfs_bridge::expression_to_rtfs_string(&expr)
        } else {
            return Err(RuntimeError::Generic(
                "Capability is missing :implementation".to_string(),
            ));
        };

        let handler_code = implementation_rtfs.clone();
        let handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync> =
            Arc::new(move |input| {
                let executor = RestrictedRtfsExecutor::new();
                executor.evaluate(&handler_code, input.clone())
            });

        let provider =
            crate::capability_marketplace::types::ProviderType::Local(LocalCapability { handler });

        let manifest_id =
            extract_string(&capability_map, "id").unwrap_or_else(|| expected_id.to_string());

        let mut manifest = CapabilityManifest::new(
            manifest_id.clone(),
            extract_required_string(&capability_map, "name")?,
            extract_required_string(&capability_map, "description")?,
            provider,
            extract_string(&capability_map, "version").unwrap_or_else(|| "0.1.0".to_string()),
        );

        if manifest_id != expected_id {
            if self.config.verbose_logging {
                eprintln!(
                    "ℹ️  LLM generated capability id '{}' differs from requested '{}'; using generated id.",
                    manifest_id, expected_id
                );
            }
        }

        manifest.permissions = extract_string_list(&capability_map, "permissions");
        manifest.effects = extract_string_list(&capability_map, "effects");
        if manifest.effects.is_empty() {
            manifest.effects.push(":pure".to_string());
        }

        if let Some(expr) = input_schema_expr {
            manifest.input_schema =
                Some(convert_expression_to_type_expr(&expr).map_err(|err| {
                    RuntimeError::Generic(format!("Invalid input schema: {}", err))
                })?);
        }
        if let Some(expr) = output_schema_expr {
            manifest.output_schema =
                Some(convert_expression_to_type_expr(&expr).map_err(|err| {
                    RuntimeError::Generic(format!("Invalid output schema: {}", err))
                })?);
        }

        manifest
            .metadata
            .insert("source".to_string(), "llm_synthesis".to_string());
        manifest.metadata.insert(
            "rtfs_implementation".to_string(),
            implementation_rtfs.clone(),
        );

        Ok(manifest)
    }

    /// Discover a capability using marketplace discovery mechanisms
    async fn discover_capability(
        &self,
        capability_id: &str,
        request: &MissingCapabilityRequest,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            quiet_eprintln!(
                "🔍 DISCOVERY: Starting fan-out discovery for '{}'",
                capability_id
            );
        }

        self.emit_event(
            capability_id,
            "discovery",
            "Starting discovery pipeline",
            None,
        );

        // Phase 2: Fan-out Discovery Pipeline
        // Try multiple discovery methods in order of preference

        // 1. Exact match in marketplace (race condition check)
        eprintln!(
            "🔍 DISCOVERY: Trying exact match in marketplace for '{}'",
            capability_id
        );
        if let Some(manifest) = self.discover_exact_match(capability_id).await? {
            eprintln!(
                "✅ DISCOVERY: Found exact match in marketplace: '{}'",
                capability_id
            );
            return Ok(Some(manifest));
        }

        // 2. Partial name matching in marketplace (skip for MCP capabilities - use discovery instead)
        if !capability_id.starts_with("mcp.") {
            eprintln!(
                "🔍 DISCOVERY: Trying partial match in marketplace for '{}'",
                capability_id
            );
            if let Some(manifest) = self.discover_partial_match(capability_id).await? {
                eprintln!(
                    "✅ DISCOVERY: Found partial match in marketplace: '{}'",
                    capability_id
                );
                return Ok(Some(manifest));
            }
        } else {
            eprintln!("🔍 DISCOVERY: Skipping partial match for MCP capability '{}' (will try MCP discovery)", capability_id);
        }

        // 3. Local manifest scanning
        eprintln!(
            "🔍 DISCOVERY: Trying local manifest scan for '{}'",
            capability_id
        );
        if let Some(manifest) = self.discover_local_manifests(capability_id).await? {
            eprintln!(
                "✅ DISCOVERY: Found in local manifests: '{}'",
                capability_id
            );
            return Ok(Some(manifest));
        }

        // 4. MCP server discovery
        eprintln!(
            "🔍 DISCOVERY: Trying MCP server discovery for '{}'",
            capability_id
        );
        match self.discover_mcp_servers(capability_id, request).await {
            Ok(Some(manifest)) => {
                eprintln!(
                    "✅ DISCOVERY: Found via MCP server discovery: '{}'",
                    capability_id
                );
                return Ok(Some(manifest));
            }
            Ok(None) => {}
            Err(e) => {
                if self.config.verbose_logging {
                    eprintln!("⚠️ MCP discovery failed (will try other methods): {}", e);
                }
            }
        }

        // 5. Web search discovery (if enabled)
        if self.feature_checker.is_web_search_enabled() {
            match self.discover_via_web_search(capability_id).await {
                Ok(Some(manifest)) => return Ok(Some(manifest)),
                Ok(None) => {}
                Err(e) => {
                    if self.config.verbose_logging {
                        eprintln!("⚠️ Web search discovery failed: {}", e);
                    }
                }
            }
        }

        // 6. Network catalog queries (if configured)
        match self.discover_network_catalogs(capability_id).await {
            Ok(Some(manifest)) => return Ok(Some(manifest)),
            Ok(None) => {}
            Err(e) => {
                if self.config.verbose_logging {
                    eprintln!("⚠️ Network catalog discovery failed: {}", e);
                }
            }
        }

        if self.config.verbose_logging {
            quiet_eprintln!("🔍 DISCOVERY: No matches found for '{}'", capability_id);
        }

        Ok(None)
    }

    /// Discover exact match in marketplace
    async fn discover_exact_match(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        self.emit_event(
            capability_id,
            "marketplace_search",
            "Checking marketplace for exact match",
            None,
        );

        // Check if capability already exists in marketplace
        let capabilities = self.marketplace.capabilities.read().await;
        quiet_eprintln!(
            "🔍 DEBUG: Checking marketplace for '{}' - found {} capabilities",
            capability_id,
            capabilities.len()
        );
        quiet_eprintln!(
            "🔍 DEBUG: Available capabilities: {:?}",
            capabilities.keys().collect::<Vec<_>>()
        );

        // Normalize the lookup key to avoid trailing/leading whitespace mismatches
        let key = capability_id.trim().trim_matches('"');
        if let Some(manifest) = capabilities.get(key) {
            quiet_eprintln!("🔍 DISCOVERY: Found exact match in marketplace: '{}'", key);
            self.emit_event(
                capability_id,
                "marketplace_search",
                format!("Exact match found: {}", manifest.id),
                None,
            );
            return Ok(Some(manifest.clone()));
        }
        quiet_eprintln!("🔍 DEBUG: No exact match found for '{}'", capability_id);
        Ok(None)
    }

    /// Discover partial matches in marketplace using name similarity
    async fn discover_partial_match(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        self.emit_event(
            capability_id,
            "marketplace_search",
            "Scanning marketplace for partial matches",
            None,
        );

        // Try to find primitive capabilities that might match
        let query = CapabilityQuery::new()
            .with_kind(CapabilityKind::Primitive)
            .with_limit(100);

        let available_capabilities = self.marketplace.list_capabilities_with_query(&query).await;

        // Simple partial matching for demonstration
        // In a real implementation, this would use more sophisticated matching
        for capability in available_capabilities {
            if self.is_partial_match(capability_id, &capability.id) {
                if self.config.verbose_logging {
                    quiet_eprintln!(
                        "🔍 DISCOVERY: Found partial match: '{}' -> '{}'",
                        capability_id,
                        capability.id
                    );
                }
                self.emit_event(
                    capability_id,
                    "marketplace_search",
                    format!("Partial match found: {}", capability.id),
                    None,
                );
                return Ok(Some(capability));
            }
        }

        Ok(None)
    }

    /// Check if two capability IDs are partial matches
    fn is_partial_match(&self, requested: &str, available: &str) -> bool {
        // Skip partial matching for MCP capabilities - we have discovery infrastructure for those
        // Partial matching should only be used for non-MCP capabilities where we need fuzzy matching
        if requested.starts_with("mcp.") || available.starts_with("mcp.") {
            return false;
        }

        // Simple partial matching logic for non-MCP capabilities
        // Check if one is a prefix of the other (only if last segments match)
        let requested_segments: Vec<&str> = requested.split('.').collect();
        let available_segments: Vec<&str> = available.split('.').collect();

        // Require at least the last segment to match for a partial match
        // This prevents "get_me" from matching "get_issues"
        if !requested_segments.is_empty() && !available_segments.is_empty() {
            let requested_last = requested_segments.last().unwrap();
            let available_last = available_segments.last().unwrap();

            // Last segments must match exactly or one must be a prefix of the other
            if requested_last == available_last
                || (requested_last.len() >= 3 && available_last.starts_with(requested_last))
                || (available_last.len() >= 3 && requested_last.starts_with(available_last))
            {
                // Also check that previous segments match
                if requested_segments.len() > 1 && available_segments.len() > 1 {
                    let req_domain = &requested_segments[..requested_segments.len() - 1];
                    let avail_domain = &available_segments[..available_segments.len() - 1];
                    if req_domain == avail_domain {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Discover capabilities from local manifest files
    async fn discover_local_manifests(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            quiet_eprintln!(
                "🔍 DISCOVERY: Scanning local manifests for '{}'",
                capability_id
            );
        }

        self.emit_event(
            capability_id,
            "local_scan",
            "Scanning local discovered manifests",
            None,
        );

        // TODO: Implement local manifest scanning
        // This would scan filesystem for capability manifest files
        // and check if any match the requested capability_id

        // For now, return None as this is not implemented
        Ok(None)
    }

    /// Discover capabilities from MCP servers via the official MCP Registry
    async fn discover_mcp_servers(
        &self,
        capability_id: &str,
        request: &MissingCapabilityRequest,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Always log MCP discovery attempts (not suppressed)
        eprintln!(
            "🔍 MCP DISCOVERY: Querying MCP Registry and overrides for '{}'",
            capability_id
        );
        if self.config.verbose_logging {
            quiet_eprintln!(
                "🔍 DISCOVERY: Querying MCP Registry for '{}'",
                capability_id
            );
        }

        self.emit_event(
            capability_id,
            "mcp_search",
            "Querying MCP registry and overrides",
            None,
        );

        let registry_client = crate::mcp::registry::MCPRegistryClient::new();

        // Try semantic search first if the query looks like a description
        let mut servers = if self.is_semantic_query(capability_id) {
            quiet_eprintln!(
                "🔍 SEMANTIC SEARCH: Detected semantic query '{}'",
                capability_id
            );
            let keywords = self.extract_search_keywords(capability_id);
            quiet_eprintln!("🔍 SEMANTIC SEARCH: Extracted keywords: {:?}", keywords);
            self.semantic_search_servers(&keywords).await?
        } else {
            // Traditional exact capability ID search
            registry_client
                .find_capability_providers(capability_id)
                .await?
        };

        // Augment with curated overrides provided by the user (if any)
        let curated = self.load_curated_overrides_for(capability_id)?;
        // Clone curated server names for auto-approval before extending servers (curated is moved below)
        let curated_server_names: HashSet<String> =
            curated.iter().map(|s| s.name.clone()).collect();
        let curated_len = curated.len();
        if !curated.is_empty() {
            eprintln!(
                "📦 MCP DISCOVERY: Loaded {} curated MCP server override(s) for '{}'",
                curated_len, capability_id
            );
            if self.config.verbose_logging {
                quiet_eprintln!(
                    "📦 DISCOVERY: Loaded {} curated MCP server override(s) for '{}'",
                    curated_len,
                    capability_id
                );
            }
            servers.extend(curated);
        }

        eprintln!(
            "🔍 MCP DISCOVERY: Found {} MCP server candidate(s) for '{}'",
            servers.len(),
            capability_id
        );
        if self.config.verbose_logging {
            quiet_eprintln!(
                "🔍 DISCOVERY: Found {} MCP servers for '{}'",
                servers.len(),
                capability_id
            );
        }

        self.emit_event(
            capability_id,
            "mcp_search",
            format!(
                "Registry and overrides yielded {} server candidate(s)",
                servers.len()
            ),
            None,
        );

        // Rank and filter servers
        let ranked_servers = self.rank_mcp_servers(capability_id, servers);

        if self.config.verbose_logging && !ranked_servers.is_empty() {
            eprintln!(
                "📊 DISCOVERY: Ranked {} server(s) with score >= 0.3 for '{}'",
                ranked_servers.len(),
                capability_id
            );
            if ranked_servers.len() <= 5 {
                for (i, ranked) in ranked_servers.iter().enumerate() {
                    eprintln!(
                        "   {}. {} (score: {:.2})",
                        i + 1,
                        ranked.server.name,
                        ranked.score
                    );
                }
            }
        }

        if !ranked_servers.is_empty() {
            self.emit_event(
                capability_id,
                "mcp_search",
                format!("{} server(s) cleared trust ranking", ranked_servers.len()),
                None,
            );
        }

        if ranked_servers.is_empty() {
            self.emit_event(
                capability_id,
                "mcp_search",
                "No MCP servers met trust requirements",
                None,
            );
            if self.config.verbose_logging {
                eprintln!(
                    "❌ DISCOVERY: No suitable MCP servers found for '{}'",
                    capability_id
                );
                eprintln!(
                    "💡 TIP: If you know the official server for this capability, you can add it to 'capabilities/mcp/overrides.json' so it's considered during discovery."
                );
            }
            return Ok(None);
        }

        // curated_server_names was already created above when loading curated overrides

        // Convert ranked servers to candidates for trust-based selection
        let candidates: Vec<ServerCandidate> = ranked_servers
            .iter()
            .map(|ranked| {
                let domain = self.extract_domain_from_server_name(&ranked.server.name);
                let repository_url = ranked
                    .server
                    .repository
                    .as_ref()
                    .map(|repo| repo.url.clone())
                    .unwrap_or_else(|| "".to_string());

                ServerCandidate::new(
                    domain,
                    ranked.server.name.clone(),
                    ranked.server.description.clone(),
                )
                .with_repository(repository_url)
                .with_score(ranked.score)
            })
            .collect();

        // Auto-approve servers from overrides in the trust registry before selection
        let mut trust_registry = self.trust_registry.clone();
        for candidate in &candidates {
            let candidate_domain = &candidate.domain;
            if curated_server_names
                .iter()
                .any(|name| self.extract_domain_from_server_name(name) == *candidate_domain)
            {
                trust_registry.approve_server(candidate_domain);
                eprintln!(
                    "✅ AUTO-APPROVE: Server '{}' from overrides.json (user-curated)",
                    candidate.domain
                );
            }
        }

        // Use trust-based server selection (servers from overrides are already approved)
        let mut selection_handler = ServerSelectionHandler::new(trust_registry);
        let selection_result = selection_handler
            .select_server(capability_id, candidates)
            .await?;

        // Find the selected server from ranked servers; if not found, reload curated overrides (user may have added one)
        let mut selected_server_opt: Option<crate::mcp::registry::McpServer> =
            ranked_servers
                .iter()
                .find(|ranked| {
                    self.extract_domain_from_server_name(&ranked.server.name)
                        == selection_result.selected_domain
                })
                .map(|r| r.server.clone());

        if selected_server_opt.is_none() {
            // Try curated overrides again to include any newly added entry during interaction
            if let Ok(curated_again) = self.load_curated_overrides_for(capability_id) {
                selected_server_opt = curated_again.into_iter().find(|srv| {
                    self.extract_domain_from_server_name(&srv.name)
                        == selection_result.selected_domain
                });
            }
        }

        let selected_server = selected_server_opt.ok_or_else(|| {
            RuntimeError::Generic("Selected server not found in candidates".to_string())
        })?;

        if self.config.verbose_logging {
            quiet_eprintln!(
                "✅ DISCOVERY: Selected MCP server '{}' for capability '{}'",
                selected_server.name,
                capability_id
            );
        }

        let remotes = if let Some(remotes) = &selected_server.remotes {
            remotes
        } else {
            eprintln!(
                "⚠️ MCP DISCOVERY: Server '{}' has no remotes; cannot introspect tools",
                selected_server.name
            );
            if self.config.verbose_logging {
                quiet_eprintln!(
                    "⚠️ DISCOVERY: Server '{}' has no remotes; cannot introspect tools",
                    selected_server.name
                );
            }
            return Ok(None);
        };

        let server_url = if let Some(url) =
            crate::mcp::registry::MCPRegistryClient::select_best_remote_url(
                remotes,
            ) {
            url
        } else {
            eprintln!(
                "⚠️ MCP DISCOVERY: No usable remote URL for server '{}'",
                selected_server.name
            );
            if self.config.verbose_logging {
                quiet_eprintln!(
                    "⚠️ DISCOVERY: No usable remote URL for server '{}'",
                    selected_server.name
                );
            }
            return Ok(None);
        };

        eprintln!(
            "🔍 MCP DISCOVERY: Introspecting server '{}' at '{}' for capability '{}'",
            selected_server.name, server_url, capability_id
        );

        self.emit_event(
            capability_id,
            "mcp_search",
            format!("Selected MCP server '{}'.", selected_server.name),
            Some(server_url.clone()),
        );

        let auth_headers = self.build_mcp_auth_headers(&selected_server.name);
        let introspector = crate::synthesis::mcp_introspector::MCPIntrospector::new();
        let introspection = introspector
            .introspect_mcp_server_with_auth(
                &server_url,
                &selected_server.name,
                auth_headers.clone(),
            )
            .await?;

        let mut manifests = introspector.create_capabilities_from_mcp(&introspection)?;

        // Optionally introspect output schemas by calling tools once (if authorized)
        // This requires auth_headers to be present (indicates we're authorized)
        if auth_headers.is_some()
            && self
                .feature_checker
                .is_output_schema_introspection_enabled()
        {
            eprintln!("🔍 Attempting output schema introspection for discovered tools...");
            for manifest in &mut manifests {
                if let Some(tool_name) = manifest.metadata.get("mcp_tool_name") {
                    // Find the corresponding tool from introspection
                    if let Some(tool) = introspection
                        .tools
                        .iter()
                        .find(|t| t.tool_name == *tool_name)
                    {
                        if let Ok((schema_opt, sample_opt)) = introspector
                            .introspect_output_schema(
                                tool,
                                &server_url,
                                &selected_server.name,
                                auth_headers.clone(),
                                None,
                            )
                            .await
                        {
                            if let Some(schema) = schema_opt {
                                manifest.output_schema = Some(schema);
                                eprintln!("✅ Updated output schema for '{}'", manifest.id);
                            }
                            if let Some(sample) = sample_opt {
                                manifest
                                    .metadata
                                    .insert("output_snippet".to_string(), sample);
                            }
                        }
                    }
                }
            }
        }
        if manifests.is_empty() {
            eprintln!(
                "⚠️ MCP DISCOVERY: Server '{}' returned no tools during introspection (might need MCP_AUTH_TOKEN)",
                selected_server.name
            );
            if self.config.verbose_logging {
                quiet_eprintln!(
                    "⚠️ DISCOVERY: Server '{}' returned no tools during introspection",
                    selected_server.name
                );
            }
            self.emit_event(
                capability_id,
                "mcp_introspection",
                "MCP server returned zero tools",
                None,
            );
            return Ok(None);
        }

        eprintln!(
            "✅ MCP DISCOVERY: Introspection found {} tool(s) from '{}'",
            manifests.len(),
            selected_server.name
        );

        self.emit_event(
            capability_id,
            "mcp_introspection",
            format!("Introspection yielded {} tool manifest(s)", manifests.len()),
            None,
        );

        let need = self.build_need_from_request(capability_id, request);
        let tool_map: HashMap<String, crate::mcp::types::DiscoveredMCPTool> =
            introspection
                .tools
                .iter()
                .cloned()
                .map(|tool| (tool.tool_name.clone(), tool))
                .collect();

        let mut candidates: Vec<ToolPromptCandidate> = Vec::new();
        for (index, manifest) in manifests.iter().enumerate() {
            if let Some(tool_name) = manifest.metadata.get("mcp_tool_name") {
                if let Some(tool) = tool_map.get(tool_name) {
                    let description = tool
                        .description
                        .clone()
                        .unwrap_or_else(|| manifest.description.clone());
                    let score =
                        self.compute_tool_score(capability_id, tool_name, &description, &need);
                    let input_keys = Self::extract_input_keys_from_type(tool.input_schema.as_ref());
                    candidates.push(ToolPromptCandidate {
                        index,
                        tool_name: tool_name.clone(),
                        description,
                        input_keys,
                        score,
                    });
                }
            }
        }

        if candidates.is_empty() {
            eprintln!(
                "⚠️ MCP DISCOVERY: No tool candidates matched for '{}' from server '{}'",
                capability_id, selected_server.name
            );
            if self.config.verbose_logging {
                eprintln!(
                    "⚠️ DISCOVERY: No tool metadata available for '{}'",
                    selected_server.name
                );
            }
            return Ok(None);
        }

        eprintln!(
            "🔍 MCP DISCOVERY: Found {} tool candidate(s) from '{}', scoring against '{}'",
            candidates.len(),
            selected_server.name,
            capability_id
        );

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Log top candidates for debugging
        if self.config.verbose_logging {
            for (i, candidate) in candidates.iter().take(3).enumerate() {
                eprintln!(
                    "  {}. {} (score: {:.2})",
                    i + 1,
                    candidate.tool_name,
                    candidate.score
                );
            }
        }

        if let Some(best) = candidates.first() {
            let overlap = Self::keyword_overlap(capability_id, &best.tool_name);
            eprintln!(
                "🔍 MCP DISCOVERY: Best match '{}' (score: {:.2}, overlap: {:.2}, threshold: >=3.0 or >=0.75)",
                best.tool_name, best.score, overlap
            );
            if best.score >= 3.0 || overlap >= 0.75 {
                if self.config.verbose_logging {
                    eprintln!(
                        "✅ DISCOVERY: Heuristic match '{}' (score {:.2}, overlap {:.2})",
                        best.tool_name, best.score, overlap
                    );
                }
                let mut manifest = manifests.swap_remove(best.index);
                self.attach_resolution_metadata(
                    &mut manifest,
                    &selected_server.name,
                    &server_url,
                    "heuristic",
                    None,
                );
                if let Err(err) = self.persist_discovered_mcp_capability(&manifest) {
                    if self.config.verbose_logging {
                        quiet_eprintln!(
                            "⚠️  Failed to persist MCP capability '{}': {}",
                            manifest.id,
                            err
                        );
                    }
                }
                self.persist_alias(ToolAliasRecord {
                    capability_pattern: capability_id.to_string(),
                    server_name: selected_server.name.clone(),
                    server_url: server_url.clone(),
                    tool_name: best.tool_name.clone(),
                    input_remap: HashMap::new(),
                });
                self.emit_event(
                    capability_id,
                    "heuristic_match",
                    format!(
                        "Selected '{}' via heuristic score {:.2}",
                        best.tool_name, best.score
                    ),
                    None,
                );
                return Ok(Some(manifest));
            }
        }

        if let Some(selection) = self
            .run_tool_selector(capability_id, &need, &candidates)
            .await?
        {
            if let Some(chosen) = candidates
                .iter()
                .find(|candidate| candidate.tool_name == selection.tool_name)
            {
                let mut manifest = manifests.swap_remove(chosen.index);
                self.attach_resolution_metadata(
                    &mut manifest,
                    &selected_server.name,
                    &server_url,
                    "llm_tool_selector",
                    Some(&selection.input_remap),
                );
                if let Err(err) = self.persist_discovered_mcp_capability(&manifest) {
                    if self.config.verbose_logging {
                        quiet_eprintln!(
                            "⚠️  Failed to persist MCP capability '{}': {}",
                            manifest.id,
                            err
                        );
                    }
                }
                self.persist_alias(ToolAliasRecord {
                    capability_pattern: capability_id.to_string(),
                    server_name: selected_server.name.clone(),
                    server_url: server_url.clone(),
                    tool_name: selection.tool_name.clone(),
                    input_remap: selection.input_remap.clone(),
                });
                self.emit_event(
                    capability_id,
                    "llm_selection",
                    format!("Registered '{}' via LLM selector", selection.tool_name),
                    None,
                );
                return Ok(Some(manifest));
            } else if self.config.verbose_logging {
                quiet_eprintln!(
                    "⚠️ DISCOVERY: Tool selector chose '{}' but it was not among candidates",
                    selection.tool_name
                );
            }
        }

        // Fallback: If LLM returned nil, find the best match considering domain keywords
        // This handles cases where LLM is too strict (e.g., rejects "list" for "get")
        // We look for a tool that shares domain keywords (like "issues") even if action verbs differ
        let capability_last = capability_id
            .split('.')
            .last()
            .unwrap_or(capability_id)
            .to_ascii_lowercase();

        // Find the best match prioritizing domain keywords over action verbs
        let mut best_fallback: Option<&ToolPromptCandidate> = None;
        let mut best_fallback_score = 0.0;

        for candidate in &candidates {
            let overlap = Self::keyword_overlap(capability_id, &candidate.tool_name);
            let tool_lower = candidate.tool_name.to_ascii_lowercase();

            // Extract domain keywords (non-verbs) from capability and tool
            let capability_tokens: HashSet<String> = capability_last
                .split(|c: char| !c.is_alphabetic())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_lowercase())
                .collect();
            let tool_tokens: HashSet<String> = tool_lower
                .split(|c: char| !c.is_alphabetic())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_lowercase())
                .collect();

            // Find shared domain keywords (excluding common action verbs)
            let action_verbs = [
                "get", "list", "fetch", "retrieve", "create", "add", "update", "delete", "remove",
            ];
            let shared_keywords: Vec<&String> = capability_tokens
                .intersection(&tool_tokens)
                .filter(|token| !action_verbs.contains(&token.as_str()))
                .collect();

            // Score based on: base score + keyword overlap + shared domain keywords
            let domain_match = !shared_keywords.is_empty();
            let fallback_score =
                candidate.score + (overlap * 2.0) + (if domain_match { 1.0 } else { 0.0 });

            // Require reasonable match: score >= 2.0 and either good overlap or domain match
            let reasonable_match = candidate.score >= 2.0 && (overlap >= 0.5 || domain_match);

            if reasonable_match && fallback_score > best_fallback_score {
                best_fallback = Some(candidate);
                best_fallback_score = fallback_score;
            }
        }

        if let Some(best) = best_fallback {
            let overlap = Self::keyword_overlap(capability_id, &best.tool_name);
            eprintln!(
                "🔄 FALLBACK: LLM returned nil, but using best semantic match '{}' (score: {:.2}, overlap: {:.2}, fallback_score: {:.2})",
                best.tool_name, best.score, overlap, best_fallback_score
            );
            let mut manifest = manifests.swap_remove(best.index);
            self.attach_resolution_metadata(
                &mut manifest,
                &selected_server.name,
                &server_url,
                "fallback_semantic_match",
                None,
            );
            if let Err(err) = self.persist_discovered_mcp_capability(&manifest) {
                if self.config.verbose_logging {
                    quiet_eprintln!(
                        "⚠️  Failed to persist MCP capability '{}': {}",
                        manifest.id,
                        err
                    );
                }
            }
            self.persist_alias(ToolAliasRecord {
                capability_pattern: capability_id.to_string(),
                server_name: selected_server.name.clone(),
                server_url: server_url.clone(),
                tool_name: best.tool_name.clone(),
                input_remap: HashMap::new(),
            });
            self.emit_event(
                capability_id,
                "fallback_match",
                format!(
                    "Selected '{}' via fallback (LLM returned nil but semantic match exists)",
                    best.tool_name
                ),
                None,
            );
            return Ok(Some(manifest));
        }

        if self.config.verbose_logging {
            quiet_eprintln!(
                "⚠️ DISCOVERY: No suitable tool selected for '{}' after heuristics and LLM selector",
                capability_id
            );
        }

        Ok(None)
    }

    /// Check if a capability name/description matches the requested capability ID
    /// Rank MCP servers by relevance and quality
    fn rank_mcp_servers(
        &self,
        capability_id: &str,
        servers: Vec<crate::mcp::registry::McpServer>,
    ) -> Vec<RankedMcpServer> {
        let mut ranked: Vec<RankedMcpServer> = servers
            .into_iter()
            .map(|server| {
                let score = self.calculate_server_score(capability_id, &server);
                RankedMcpServer { server, score }
            })
            .collect();

        // Sort by score (highest first)
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Filter out servers with very low scores
        ranked
            .into_iter()
            .filter(|ranked| ranked.score >= 0.3) // Minimum threshold
            .collect()
    }

    /// Calculate a relevance score for an MCP server
    fn calculate_server_score(
        &self,
        capability_id: &str,
        server: &crate::mcp::registry::McpServer,
    ) -> f64 {
        let mut score = 0.0;
        let requested_lower = capability_id.to_lowercase();
        let name_lower = server.name.to_lowercase();
        let desc_lower = server.description.to_lowercase();

        // Exact name match (highest priority)
        if name_lower == requested_lower {
            score += 10.0;
        }
        // Exact match in description
        else if desc_lower.contains(&format!(" {} ", requested_lower)) {
            score += 8.0;
        }
        // Partial name match
        else if name_lower.contains(&requested_lower) {
            score += 6.0;
        }
        // Reverse partial match (requested contains server name)
        else if requested_lower.contains(&name_lower) {
            score += 4.0;
        }
        // Description contains capability
        else if desc_lower.contains(&requested_lower) {
            score += 3.0;
        }

        // Penalize overly specific servers (plugins, extensions, etc.)
        if name_lower.contains("plugin")
            || name_lower.contains("extension")
            || name_lower.contains("specific")
            || name_lower.contains("custom")
        {
            score -= 2.0;
        }

        // Bonus for general-purpose servers
        if name_lower.contains("api")
            || name_lower.contains("sdk")
            || name_lower.contains("client")
            || name_lower.contains("service")
            || name_lower.contains("provider")
        {
            score += 1.0;
        }

        // Bonus for official/well-known providers (generic pattern matching)
        if self.is_official_provider(&name_lower, &requested_lower) {
            score += 2.0;
        }

        // Additional boost for servers that look official/curated based on repository or packages
        if requested_lower.contains("github") {
            // Repository URL pointing to GitHub orgs that likely indicate officialness
            if let Some(repo) = &server.repository {
                let repo_url_lower = repo.url.to_lowercase();
                if repo_url_lower.contains("github.com") {
                    // Mild boost for any GitHub-hosted repo
                    score += 1.0;
                    // Extra boost if under the github org or clearly official
                    if repo_url_lower.contains("github.com/github/") {
                        score += 1.0;
                    }
                }
            }

            // NPM packages under @github scope or identifiers containing github
            if let Some(packages) = &server.packages {
                if packages
                    .iter()
                    .any(|p| p.identifier.to_lowercase().starts_with("@github/"))
                {
                    score += 1.0;
                } else if packages
                    .iter()
                    .any(|p| p.identifier.to_lowercase().contains("github"))
                {
                    score += 0.5;
                }
            }

            // Name hints like "mcp" and provider name
            if name_lower.contains("mcp") {
                score += 0.5;
            }
        }

        // Normalize score to 0-1 range
        (score / 10.0f64).min(1.0f64).max(0.0f64)
    }

    /// Check if a server appears to be an official provider for the requested capability
    fn is_official_provider(&self, server_name: &str, requested_capability: &str) -> bool {
        // Extract domain/service name from requested capability
        let requested_parts: Vec<&str> = requested_capability.split('.').collect();
        if requested_parts.is_empty() {
            return false;
        }

        // Check if server name contains the main domain/service
        let main_domain = requested_parts[0];
        server_name.contains(main_domain)
    }

    /// Check if a query looks like a semantic description rather than a capability ID
    fn is_semantic_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();

        // Check for semantic indicators
        let semantic_indicators = [
            "official",
            "api",
            "service",
            "client",
            "sdk",
            "provider",
            "weather",
            "github",
            "database",
            "payment",
            "email",
            "storage",
            "authentication",
            "analytics",
            "monitoring",
            "logging",
        ];

        // If it contains spaces or semantic keywords, treat as semantic query
        query.contains(' ') ||
        semantic_indicators.iter().any(|indicator| query_lower.contains(indicator)) ||
        // If it doesn't look like a capability ID (no dots, not camelCase)
        (!query.contains('.') && !query.chars().any(|c| c.is_uppercase()))
    }

    /// Extract semantic keywords from a capability search query
    fn extract_search_keywords(&self, query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();
        let mut keywords = Vec::new();

        // Split by common separators
        let parts: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '.' || c == '-' || c == '_')
            .filter(|part| !part.is_empty() && part.len() > 1)
            .collect();

        // Add all parts as keywords
        keywords.extend(parts.iter().map(|s| s.to_string()));

        // Add common API/service keywords if not present
        let api_keywords = ["api", "service", "client", "sdk", "official", "provider"];
        for keyword in &api_keywords {
            if !keywords.contains(&keyword.to_string()) && query_lower.contains(keyword) {
                keywords.push(keyword.to_string());
            }
        }

        keywords
    }

    /// Perform semantic search across MCP servers using keywords
    async fn semantic_search_servers(
        &self,
        keywords: &[String],
    ) -> RuntimeResult<Vec<crate::mcp::registry::McpServer>> {
        let registry_client = crate::mcp::registry::MCPRegistryClient::new();
        let mut all_servers = Vec::new();

        // Search for each keyword
        for keyword in keywords {
            match registry_client.find_capability_providers(keyword).await {
                Ok(servers) => {
                    all_servers.extend(servers);
                }
                Err(e) => {
                    eprintln!(
                        "⚠️ SEMANTIC SEARCH: Failed to search for '{}': {}",
                        keyword, e
                    );
                }
            }
        }

        // Remove duplicates based on server name
        let mut unique_servers = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for server in all_servers {
            if seen_names.insert(server.name.clone()) {
                unique_servers.push(server);
            }
        }

        Ok(unique_servers)
    }

    /// Extract domain from server name (fallback when URL is not available)
    fn extract_domain_from_server_name(&self, name: &str) -> String {
        // Try to extract domain from server name patterns
        // e.g., "ai.smithery/Hint-Services-obsidian-github-mcp" -> "ai.smithery"
        if let Some(slash_pos) = name.find('/') {
            name[..slash_pos].to_string()
        } else if let Some(dot_pos) = name.find('.') {
            // If it contains dots, use the first part
            name[..dot_pos].to_string()
        } else {
            // Fallback to the full name
            name.to_string()
        }
    }

    /// Load curated MCP server overrides from a local JSON file and select those matching the capability id
    pub(crate) fn load_curated_overrides_for(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Vec<crate::mcp::registry::McpServer>> {
        use std::fs;
        use std::path::Path;

        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        // Try workspace root 'capabilities/mcp/overrides.json'. If we are inside ccos, go up one level
        let overrides_path = if root.ends_with("ccos") {
            root.parent()
                .unwrap_or(&root)
                .join("capabilities/mcp/overrides.json")
        } else {
            root.join("capabilities/mcp/overrides.json")
        };

        if !Path::new(&overrides_path).exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&overrides_path).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read curated overrides file '{}': {}",
                overrides_path.display(),
                e
            ))
        })?;

        let parsed: CuratedOverrides = serde_json::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to parse curated overrides JSON '{}': {}",
                overrides_path.display(),
                e
            ))
        })?;

        let mut matched = Vec::new();
        for entry in parsed.entries.iter() {
            if entry
                .matches
                .iter()
                .any(|pat| Self::pattern_match(pat, capability_id))
            {
                matched.push(entry.server.clone());
            }
        }

        Ok(matched)
    }

    /// Simple wildcard pattern matching supporting:
    /// - exact match
    /// - suffix '*' (prefix match)
    /// - '*' anywhere (contains match)
    fn pattern_match(pattern: &str, text: &str) -> bool {
        if pattern == text {
            return true;
        }
        if let Some(idx) = pattern.find('*') {
            let (pre, post) = pattern.split_at(idx);
            let post = &post[1..];
            if pre.is_empty() && post.is_empty() {
                return true;
            }
            let starts_ok = pre.is_empty() || text.starts_with(pre);
            let ends_ok = post.is_empty() || text.ends_with(post);
            if starts_ok && ends_ok {
                return true;
            }
            // Fallback contains when '*' in middle
            let needle = format!("{}{}", pre, post);
            return text.contains(&needle);
        }
        false
    }

    fn is_capability_match(
        &self,
        requested: &str,
        capability_name: &str,
        capability_desc: &str,
    ) -> bool {
        let requested_lower = requested.to_lowercase();
        let name_lower = capability_name.to_lowercase();
        let desc_lower = capability_desc.to_lowercase();

        // Exact match
        if name_lower == requested_lower {
            return true;
        }

        // Partial match in capability name
        if name_lower.contains(&requested_lower) || requested_lower.contains(&name_lower) {
            return true;
        }

        // Check if requested capability is mentioned in description
        if desc_lower.contains(&requested_lower) {
            return true;
        }

        // Check for domain-based matching (e.g., "travel.flights" matches "flights")
        let requested_parts: Vec<&str> = requested.split('.').collect();
        if requested_parts.len() > 1 {
            let last_part = requested_parts.last().unwrap().to_lowercase();
            if name_lower.contains(&last_part) {
                return true;
            }
        }

        false
    }

    /// Discover capabilities via web search
    async fn discover_via_web_search(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!("🔍 DISCOVERY: Searching web for '{}'", capability_id);
        }

        // Use web search to find API specs and documentation
        let mut web_searcher =
            crate::synthesis::web_search_discovery::WebSearchDiscovery::new("auto".to_string());

        // Search for OpenAPI specs, API docs, etc.
        let search_results = web_searcher.search_for_api_specs(capability_id).await?;

        if search_results.is_empty() {
            if self.config.verbose_logging {
                eprintln!(
                    "🔍 DISCOVERY: No web search results found for '{}'",
                    capability_id
                );
            }
            return Ok(None);
        }

        // Try to convert the best result to a capability manifest
        if let Some(best_result) = search_results.first() {
            if self.config.verbose_logging {
                eprintln!(
                    "🔍 DISCOVERY: Found web result: {} ({})",
                    best_result.title, best_result.result_type
                );
            }

            // Try to import as OpenAPI if it's an OpenAPI spec
            if best_result.result_type == "openapi_spec" {
                return self
                    .import_openapi_from_url(&best_result.url, capability_id)
                    .await;
            }

            // Try to import as generic HTTP API
            return self
                .import_http_api_from_url(&best_result.url, capability_id)
                .await;
        }

        Ok(None)
    }

    /// Import OpenAPI spec from URL
    async fn import_openapi_from_url(
        &self,
        url: &str,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!(
                "📥 DISCOVERY: Attempting to import OpenAPI spec from: {}",
                url
            );
        }

        // Create OpenAPI importer
        let importer = crate::synthesis::openapi_importer::OpenAPIImporter::new(url.to_string());

        // Create complete RTFS capability from OpenAPI spec
        match importer.create_rtfs_capability(url, capability_id).await {
            Ok(manifest) => {
                if self.config.verbose_logging {
                    eprintln!(
                        "✅ DISCOVERY: Created RTFS capability from OpenAPI: {}",
                        manifest.id
                    );
                    if let Some(rtfs_code) = manifest.metadata.get("rtfs_code") {
                        eprintln!("📝 RTFS Code generated ({} chars)", rtfs_code.len());
                    }
                }
                Ok(Some(manifest))
            }
            Err(e) => {
                if self.config.verbose_logging {
                    eprintln!(
                        "⚠️ DISCOVERY: Failed to create RTFS capability from OpenAPI: {}",
                        e
                    );
                }
                Ok(None)
            }
        }
    }

    /// Import generic HTTP API from URL
    async fn import_http_api_from_url(
        &self,
        url: &str,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!("📥 DISCOVERY: Attempting to import HTTP API from: {}", url);
        }

        let base_url = Self::infer_base_url(url);

        // Try multi-capability synthesis first for APIs that support it
        if let Ok(Some(multi_manifests)) = self
            .attempt_multi_capability_synthesis(
                capability_id,
                &format!("API documentation from {}", url),
                &base_url,
            )
            .await
        {
            if self.config.verbose_logging {
                eprintln!(
                    "✅ DISCOVERY: Multi-capability synthesis generated {} capabilities",
                    multi_manifests.len()
                );
            }
            // Return the first capability as the primary one
            return Ok(multi_manifests.into_iter().next());
        }

        // Fall back to single generic HTTP API capability
        let provider_slug = Self::infer_provider_slug(capability_id, &base_url);
        let env_var_name = Self::env_var_name_for_slug(&provider_slug);
        let primary_query_param =
            Self::infer_primary_query_param(&provider_slug, capability_id, &base_url);
        let fallback_query_param = Self::infer_secondary_query_param(&primary_query_param);

        // Create a generic HTTP API capability
        let manifest = crate::capability_marketplace::types::CapabilityManifest {
            id: capability_id.to_string(),
            name: format!("{} API", capability_id),
            description: format!("HTTP API discovered from {}", url),
            version: "1.0.0".to_string(),
            provider: crate::capability_marketplace::types::ProviderType::Http(
                crate::capability_marketplace::types::HttpCapability {
                    base_url: base_url.clone(),
                    auth_token: None,
                    timeout_ms: 30000,
                },
            ),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "web_search_discovery".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("web_{}", url.replace("/", "_")),
                custody_chain: vec!["web_search".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec!["network.http".to_string()],
            effects: vec!["network_request".to_string()],
            metadata: {
                let mut meta = std::collections::HashMap::new();
                meta.insert("discovery_method".to_string(), "web_search".to_string());
                meta.insert("source_url".to_string(), url.to_string());
                meta.insert("base_url".to_string(), base_url.clone());
                meta.insert("provider_slug".to_string(), provider_slug.clone());
                meta.insert("api_type".to_string(), "http_rest".to_string());
                meta.insert("auth_env_var".to_string(), env_var_name.clone());
                meta.insert("auth_query_param".to_string(), primary_query_param.clone());
                meta.insert(
                    "auth_secondary_query_param".to_string(),
                    fallback_query_param.clone(),
                );
                meta
            },
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
        };

        if self.config.verbose_logging {
            eprintln!(
                "✅ DISCOVERY: Created generic HTTP API capability: {}",
                manifest.id
            );
        }

        // Save the capability to storage
        self.save_generic_capability(&manifest, url).await?;

        Ok(Some(manifest))
    }

    async fn attempt_pure_rtfs_generation(
        &self,
        request: &MissingCapabilityRequest,
        capability_id_normalized: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Build a local pure-RTFS generator and ask it for an implementation
        let config = MissingCapabilityStrategyConfig::default();
        let strategy = PureRtfsGenerationStrategy::new(config);

        match strategy.generate_pure_rtfs_implementation(request).await {
            Ok(rtfs_source) => {
                self.process_generated_rtfs(rtfs_source, capability_id_normalized, "pure_rtfs_generated").await
            }
            Err(_) => Ok(None),
        }
    }

    /// Attempt user interaction strategy
    pub async fn attempt_user_interaction(
        &self,
        request: &MissingCapabilityRequest,
        _capability_id_normalized: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        let config = MissingCapabilityStrategyConfig::default();
        let strategy = UserInteractionStrategy::new(config)
            .with_marketplace(self.marketplace.clone());
        
        let context = crate::planner::modular_planner::ResolutionContext {
            domain_hints: vec![],
            resolved_capabilities: Default::default(),
            preferences: Default::default(),
            allow_synthesis: true,
            ambiguity_threshold: 0.5,
        };

        // User interaction currently just prints to stdout/stderr and simulates choices
        // It doesn't return a manifest directly in the current implementation
        if let Ok(ResolutionResult::Resolved { .. }) = strategy.resolve(request, &context).await {
             // If user provided a solution that resolved it, we might need to fetch it.
             // But the current implementation returns NotFound(Not Implemented).
             // If we implement it fully, we would return the manifest here.
             Ok(None)
        } else {
             Ok(None)
        }
    }

    async fn attempt_external_llm_hint(
        &self,
        request: &MissingCapabilityRequest,
        capability_id_normalized: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        let config = MissingCapabilityStrategyConfig::default();
        let mut strategy = ExternalLlmHintStrategy::new(config);
        
        // Inject arbiter if available
        {
            let guard = self.delegating_arbiter.read().unwrap();
            if let Some(arbiter) = guard.as_ref() {
                strategy = strategy.with_arbiter(arbiter.clone());
            } else {
                return Ok(None);
            }
        }

        match strategy.generate_implementation(request).await {
            Ok(rtfs_source) => {
                self.process_generated_rtfs(rtfs_source, capability_id_normalized, "llm_generated").await
            }
            Err(_) => Ok(None),
        }
    }

    async fn attempt_service_discovery_hint(
        &self,
        request: &MissingCapabilityRequest,
        _capability_id_normalized: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        let config = MissingCapabilityStrategyConfig::default();
        let strategy = ServiceDiscoveryHintStrategy::new(config);
        
        let context = crate::planner::modular_planner::ResolutionContext {
            domain_hints: vec![],
            resolved_capabilities: Default::default(),
            preferences: Default::default(),
            allow_synthesis: true,
            ambiguity_threshold: 0.5,
        };

        // Currently just a placeholder/log
        let _ = strategy.resolve(request, &context).await;
        Ok(None)
    }

    async fn process_generated_rtfs(
        &self,
        rtfs_source: String,
        capability_id_normalized: &str,
        source_label: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Try to parse a capability manifest from the generated RTFS
        match self
            .manifest_from_rtfs(&rtfs_source, capability_id_normalized)
            .await
        {
            Ok(mut manifest) => {
                // Mark the generated RTFS implementation for persistence & auditing
                manifest
                    .metadata
                    .insert("rtfs_implementation".to_string(), rtfs_source.clone());
                manifest.metadata.insert(
                    "resolution_source".to_string(),
                    source_label.to_string(),
                );

                // Persist generated RTFS to disk so it's available for inspection
                if let Ok(path) = self.persist_llm_generated_capability(&manifest).await {
                    manifest
                        .metadata
                        .insert("storage_path".to_string(), path.display().to_string());
                }

                Ok(Some(manifest))
            }
            Err(err) => {
                if self.config.verbose_logging {
                    eprintln!(
                        "❌ RTFS generation produced invalid capability RTFS: {}",
                        err
                    );
                }
                Ok(None)
            }
        }
    }

    /// Save a generic HTTP API capability to storage
    async fn save_generic_capability(
        &self,
        manifest: &CapabilityManifest,
        source_url: &str,
    ) -> RuntimeResult<()> {
        let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("./capabilities"));

        std::fs::create_dir_all(&storage_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create storage directory: {}", e))
        })?;

        let capability_dir = storage_dir.join(&manifest.id);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        // TODO: Save the actual RTFS implementation code
        // For now, just create a placeholder file
        let capability_file = capability_dir.join("capability.rtfs");
        let source_url = manifest
            .metadata
            .get("source_url")
            .cloned()
            .or_else(|| manifest.metadata.get("base_url").cloned())
            .unwrap_or_default();
        let placeholder_content = format!(
            r#"(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :source_url "{}"
  :discovery_method "multi_capability_synthesis"
  :created_at "{}"
  :capability_type "specialized_http_api"
  :permissions [:network.http]
  :effects [:network_request]
  :input-schema :any
  :output-schema :any
  :implementation
    (do
      ;; TODO: Generated RTFS implementation will be saved here
      (call "ccos.io.println" "Multi-capability synthesis placeholder for {}")
      {{:status "placeholder" :capability_id "{}"}})
)"#,
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.description,
            source_url,
            chrono::Utc::now().to_rfc3339(),
            manifest.id,
            manifest.id
        );

        std::fs::write(&capability_file, placeholder_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write capability file: {}", e))
        })?;

        if self.config.verbose_logging {
            eprintln!("💾 MULTI-CAPABILITY: Saved capability: {}", manifest.id);
        }

        Ok(())
    }

    /// Convert JSON schema to RTFS schema string
    fn json_schema_to_rtfs(&self, json_schema: Option<&serde_json::Value>) -> String {
        match json_schema {
            Some(schema) => {
                // Convert JSON schema to RTFS format
                // For now, create a simple RTFS schema based on the JSON structure
                if let Some(properties) = schema.get("properties") {
                    let mut rtfs_props = Vec::new();
                    for (key, prop) in properties.as_object().unwrap_or(&serde_json::Map::new()) {
                        if let Some(prop_type) = prop.get("type") {
                            let rtfs_type = match prop_type.as_str().unwrap_or("string") {
                                "string" => ":string",
                                "number" => ":number",
                                "boolean" => ":boolean",
                                "array" => ":vector",
                                "object" => ":map",
                                _ => ":any",
                            };
                            rtfs_props.push(format!(":{} {}", key, rtfs_type));
                        }
                    }
                    if rtfs_props.is_empty() {
                        ":any".to_string()
                    } else {
                        format!("(map {})", rtfs_props.join(" "))
                    }
                } else {
                    ":any".to_string()
                }
            }
            None => ":any".to_string(),
        }
    }

    /// Save a multi-capability synthesis result with actual RTFS implementation code
    async fn save_multi_capability_with_code(
        &self,
        manifest: &CapabilityManifest,
        implementation_code: &str,
        base_url: &str,
        endpoint: &MultiCapabilityEndpoint,
    ) -> RuntimeResult<std::path::PathBuf> {
        let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                // Get the project root directory (parent of rtfs_compiler)
                let current_dir =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                if current_dir.ends_with("rtfs_compiler") {
                    current_dir
                        .parent()
                        .unwrap_or(&current_dir)
                        .join("capabilities")
                } else {
                    current_dir.join("capabilities")
                }
            });

        std::fs::create_dir_all(&storage_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create storage directory: {}", e))
        })?;

        let capability_dir = storage_dir.join(&manifest.id);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        // Convert JSON schemas to RTFS format
        let input_schema_rtfs = self.json_schema_to_rtfs(endpoint.input_schema.as_ref());
        let output_schema_rtfs = self.json_schema_to_rtfs(endpoint.output_schema.as_ref());

        // Create the full capability definition with the generated RTFS implementation
        let capability_file = capability_dir.join("capability.rtfs");
        let full_capability_content = format!(
            r#"(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :source_url "{}"
  :discovery_method "multi_capability_synthesis"
  :created_at "{}"
  :capability_type "specialized_http_api"
  :permissions [:network.http]
  :effects [:network_request]
  :input-schema {}
  :output-schema {}
  :implementation
    {}
)"#,
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.description,
            base_url,
            chrono::Utc::now().to_rfc3339(),
            input_schema_rtfs,
            output_schema_rtfs,
            implementation_code
        );

        std::fs::write(&capability_file, full_capability_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write capability file: {}", e))
        })?;

        if self.config.verbose_logging {
            eprintln!(
                "💾 MULTI-CAPABILITY: Saved capability with RTFS implementation: {} ({})",
                manifest.id,
                capability_file.display()
            );
        }

        Ok(capability_file)
    }
}

fn extract_rtfs_block(response: &str) -> String {
    CODE_BLOCK_RE
        .captures(response)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| response.trim().to_string())
}

fn should_log_debug_prompts() -> bool {
    matches!(
        std::env::var("CCOS_DEBUG_PROMPTS")
            .or_else(|_| std::env::var("RTFS_DEBUG_PROMPTS"))
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn sanitize_value(value: &Value) -> String {
    let mut repr = format!("{}", value);
    if repr.len() > 200 {
        repr.truncate(197);
        repr.push_str("...");
    }
    repr
}

fn keyword_matches(key: &MapKey, needle: &str) -> bool {
    match key {
        MapKey::Keyword(keyword) => keyword.0 == needle,
        MapKey::String(s) => s.trim_start_matches(':') == needle,
        _ => false,
    }
}

fn extract_string(map: &HashMap<MapKey, Expression>, key: &str) -> Option<String> {
    map.iter()
        .find(|(k, _)| keyword_matches(k, key))
        .and_then(|(_, value)| match value {
            Expression::Literal(Literal::String(s)) => Some(s.clone()),
            _ => None,
        })
}

fn extract_required_string(map: &HashMap<MapKey, Expression>, key: &str) -> RuntimeResult<String> {
    extract_string(map, key)
        .ok_or_else(|| RuntimeError::Generic(format!("Capability definition missing :{}", key)))
}

fn extract_string_list(map: &HashMap<MapKey, Expression>, key: &str) -> Vec<String> {
    map.iter()
        .find(|(k, _)| keyword_matches(k, key))
        .and_then(|(_, value)| match value {
            Expression::Vector(items) => Some(
                items
                    .iter()
                    .filter_map(|expr| {
                        if let Expression::Literal(Literal::String(s)) = expr {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn convert_expression_to_type_expr(expr: &Expression) -> Result<TypeExpr, String> {
    match expression_ast_to_type_expr(expr) {
        Ok(type_expr) => Ok(type_expr),
        Err(_) => {
            let schema_src = expression_to_rtfs_string(expr);
            match std::panic::catch_unwind(|| parse_type_expression(&schema_src)) {
                Ok(Ok(type_expr)) => Ok(type_expr),
                Ok(Err(err)) => {
                    eprintln!(
                        "⚠️  Type expression parse error for schema '{}': {:?}; defaulting to :any",
                        schema_src, err
                    );
                    Ok(TypeExpr::Any)
                }
                Err(_) => {
                    eprintln!(
                        "⚠️  Type expression parsing panicked for schema '{}'; defaulting to :any",
                        schema_src
                    );
                    Ok(TypeExpr::Any)
                }
            }
        }
    }
}

fn expression_ast_to_type_expr(expr: &Expression) -> Result<TypeExpr, String> {
    match expr {
        Expression::Literal(literal) => match literal {
            Literal::Keyword(k) => {
                if let Some(kind) = keyword_to_type_expr(&k.0) {
                    Ok(kind)
                } else {
                    Ok(TypeExpr::Primitive(PrimitiveType::Custom(RtfsKeyword(
                        value_conversion::map_key_to_string(&rtfs::ast::MapKey::Keyword(k.clone())),
                    ))))
                }
            }
            Literal::Symbol(s) => Ok(TypeExpr::Alias(RtfsSymbol(s.0.clone()))),
            _ => Ok(TypeExpr::Literal(literal.clone())),
        },
        Expression::Vector(items) => sequence_type_expr(items),
        Expression::List(items) => sequence_type_expr(items),
        _ => Err("Unsupported expression form for type expression".to_string()),
    }
}

fn sequence_type_expr(items: &[Expression]) -> Result<TypeExpr, String> {
    if items.is_empty() {
        return Err("Empty sequence cannot represent a type expression".to_string());
    }

    if items.len() == 1 {
        return expression_ast_to_type_expr(&items[0]);
    }

    let head_name = expr_to_symbol_name(&items[0])
        .ok_or_else(|| "Type expression head must be a keyword or symbol".to_string())?;

    match head_name.as_str() {
        "vector" | "seq" => {
            let inner = items
                .get(1)
                .ok_or_else(|| "Vector type missing element type".to_string())?;
            Ok(TypeExpr::Vector(Box::new(expression_ast_to_type_expr(
                inner,
            )?)))
        }
        "tuple" => {
            let mut types = Vec::new();
            for item in &items[1..] {
                types.push(expression_ast_to_type_expr(item)?);
            }
            Ok(TypeExpr::Tuple(types))
        }
        "map" => {
            let (entries, wildcard) = parse_map_entries(&items[1..])?;
            Ok(TypeExpr::Map { entries, wildcard })
        }
        "optional" => {
            let inner = items
                .get(1)
                .ok_or_else(|| "Optional type missing inner type".to_string())?;
            Ok(TypeExpr::Optional(Box::new(expression_ast_to_type_expr(
                inner,
            )?)))
        }
        "union" => {
            let mut types = Vec::new();
            for item in &items[1..] {
                types.push(expression_ast_to_type_expr(item)?);
            }
            Ok(TypeExpr::Union(types))
        }
        "intersection" | "and" => {
            let mut types = Vec::new();
            for item in &items[1..] {
                types.push(expression_ast_to_type_expr(item)?);
            }
            Ok(TypeExpr::Intersection(types))
        }
        "enum" => {
            let mut values = Vec::new();
            for value in &items[1..] {
                if let Expression::Literal(lit) = value {
                    values.push(lit.clone());
                } else {
                    return Err("Enum expects literal values".to_string());
                }
            }
            Ok(TypeExpr::Enum(values))
        }
        "literal" | "val" => {
            let value_expr = items
                .get(1)
                .ok_or_else(|| "Literal type missing value".to_string())?;
            if let Expression::Literal(lit) = value_expr {
                Ok(TypeExpr::Literal(lit.clone()))
            } else {
                Err("Literal type value must be a literal".to_string())
            }
        }
        "resource" => {
            let target = items
                .get(1)
                .ok_or_else(|| "Resource type missing identifier".to_string())?;
            let name = expr_to_symbol_name(target)
                .ok_or_else(|| "Resource identifier must be a symbol".to_string())?;
            Ok(TypeExpr::Resource(RtfsSymbol(name)))
        }
        _ => expression_ast_to_type_expr(&items[0]),
    }
}

fn parse_map_entries(
    entries: &[Expression],
) -> Result<(Vec<MapTypeEntry>, Option<Box<TypeExpr>>), String> {
    let mut result = Vec::new();
    let mut wildcard: Option<Box<TypeExpr>> = None;

    for entry in entries {
        let seq = expression_sequence_items(entry)
            .ok_or_else(|| "Map type entries must be vectors or lists".to_string())?;
        if seq.is_empty() {
            continue;
        }

        let key_name = expr_to_symbol_name(&seq[0])
            .ok_or_else(|| "Map entry key must be a keyword".to_string())?;

        if key_name == "*" {
            let value_expr = seq
                .get(1)
                .ok_or_else(|| "Wildcard map entry missing value type".to_string())?;
            let value_type = expression_ast_to_type_expr(value_expr)?;
            wildcard = Some(Box::new(value_type));
            continue;
        }

        let value_expr = seq
            .get(1)
            .ok_or_else(|| format!("Map entry '{}' missing value type", key_name))?;
        let mut value_type = expression_ast_to_type_expr(value_expr)?;
        let mut optional = false;

        if let TypeExpr::Optional(inner) = value_type {
            optional = true;
            value_type = *inner;
        }

        result.push(MapTypeEntry {
            key: RtfsKeyword(key_name),
            value_type: Box::new(value_type),
            optional,
        });
    }

    Ok((result, wildcard))
}

fn expression_sequence_items(expr: &Expression) -> Option<&[Expression]> {
    match expr {
        Expression::Vector(items) => Some(items.as_slice()),
        Expression::List(items) => Some(items.as_slice()),
        _ => None,
    }
}

fn expr_to_symbol_name(expr: &Expression) -> Option<String> {
    match expr {
        Expression::Literal(Literal::Keyword(k)) => {
            Some(value_conversion::map_key_to_string(&rtfs::ast::MapKey::Keyword(k.clone())))
        }
        Expression::Literal(Literal::Symbol(s)) => Some(s.0.clone()),
        _ => None,
    }
}

fn keyword_to_type_expr(name: &str) -> Option<TypeExpr> {
    match name {
        "string" | "str" => Some(TypeExpr::Primitive(PrimitiveType::String)),
        "int" | "integer" => Some(TypeExpr::Primitive(PrimitiveType::Int)),
        "float" | "double" | "number" => Some(TypeExpr::Primitive(PrimitiveType::Float)),
        "bool" | "boolean" => Some(TypeExpr::Primitive(PrimitiveType::Bool)),
        "nil" | "null" => Some(TypeExpr::Primitive(PrimitiveType::Nil)),
        "keyword" => Some(TypeExpr::Primitive(PrimitiveType::Keyword)),
        "symbol" => Some(TypeExpr::Primitive(PrimitiveType::Symbol)),
        "any" => Some(TypeExpr::Any),
        "never" => Some(TypeExpr::Never),
        _ => None,
    }
}

fn extract_capability_rtfs_from_response(response: &str) -> Option<String> {
    if let Some(caps) = CODE_BLOCK_RE.captures(response) {
        let code = caps.get(1).unwrap().as_str().trim();
        if code.starts_with("(capability") || code.starts_with('{') {
            return Some(code.to_string());
        }
    }

    let trimmed = response.trim().trim_matches('`').trim();
    if trimmed.starts_with("(capability") || trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }

    None
}

impl MissingCapabilityResolver {
    pub fn get_stats(&self) -> ResolverStats {
        let queue = self.queue.lock().unwrap();
        let q_stats = queue.stats();
        ResolverStats {
            pending_count: q_stats.pending_count,
            in_progress_count: q_stats.in_progress_count,
            resolved_count: 0, // Not tracked in queue stats currently
            failed_count: q_stats.failed_count,
        }
    }

    /// Get the checkpoint archive reference
    pub fn get_checkpoint_archive(&self) -> Arc<CheckpointArchive> {
        Arc::clone(&self.checkpoint_archive)
    }

    /// Get queue statistics (for compatibility with calculate_success_rate)
    pub fn get_queue_stats(&self) -> QueueStats {
        let queue = self.queue.lock().unwrap();
        queue.stats()
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Resolution History & Backoff Methods (unified from continuous_resolution.rs)
    // ─────────────────────────────────────────────────────────────────────────────

    /// Calculate exponential backoff delay based on attempt count
    pub fn calculate_backoff_delay(&self, attempt_count: u32) -> u64 {
        let base = self.config.base_backoff_seconds;
        let max = self.config.max_backoff_seconds;
        // Exponential backoff: base * 2^(attempt_count - 1), capped at max
        let delay = base.saturating_mul(1u64 << attempt_count.saturating_sub(1).min(10));
        delay.min(max)
    }

    /// Record a resolution attempt for a capability
    pub fn record_resolution_attempt(&self, attempt: ResolutionAttempt) {
        let mut history = self.resolution_history.write().unwrap();
        history
            .entry(attempt.capability_id.clone())
            .or_insert_with(Vec::new)
            .push(attempt);
    }

    /// Get the resolution history for a specific capability
    pub fn get_resolution_history(&self, capability_id: &str) -> Vec<ResolutionAttempt> {
        let history = self.resolution_history.read().unwrap();
        history.get(capability_id).cloned().unwrap_or_default()
    }

    /// Get the number of resolution attempts for a capability
    pub fn get_attempt_count(&self, capability_id: &str) -> u32 {
        let history = self.resolution_history.read().unwrap();
        history
            .get(capability_id)
            .map(|attempts| attempts.len() as u32)
            .unwrap_or(0)
    }

    /// Check if a capability has exceeded max retry attempts
    pub fn has_exceeded_max_attempts(&self, capability_id: &str) -> bool {
        self.get_attempt_count(capability_id) >= self.config.max_attempts
    }

    /// Assess risk level for resolving a capability
    pub fn assess_risk(&self, capability_id: &str) -> RiskAssessment {
        let attempt_count = self.get_attempt_count(capability_id);
        let history = self.get_resolution_history(capability_id);

        // Collect risk factors first to determine priority
        let mut risk_factors = Vec::new();
        if attempt_count > 2 {
            risk_factors.push(format!("Multiple prior failures ({})", attempt_count));
        }
        if history.iter().any(|a| {
            a.error_message
                .as_ref()
                .map(|e| e.contains("timeout"))
                .unwrap_or(false)
        }) {
            risk_factors.push("Previous timeout errors".to_string());
        }

        // Security concerns for sensitive capabilities
        let security_concerns = if capability_id.contains("exec")
            || capability_id.contains("shell")
            || capability_id.contains("system")
        {
            vec!["Capability may execute arbitrary code".to_string()]
        } else if capability_id.contains("admin") || capability_id.contains("root") {
            vec!["High privilege access required".to_string()]
        } else {
            Vec::new()
        };

        // Compliance requirements based on capability type
        let compliance_requirements = if capability_id.contains("data")
            || capability_id.contains("pii")
            || capability_id.contains("personal")
        {
            vec!["Data handling compliance required".to_string()]
        } else {
            Vec::new()
        };

        // Determine priority based on capability name patterns and collected concerns
        let priority = if capability_id.contains("security")
            || capability_id.contains("auth")
            || capability_id.contains("crypto")
        {
            ResolutionPriority::Critical
        } else if capability_id.contains("core") || capability_id.contains("system") {
            ResolutionPriority::High
        } else if security_concerns.len() > 1 || compliance_requirements.len() > 1 {
            ResolutionPriority::Critical
        } else if !security_concerns.is_empty() || !compliance_requirements.is_empty() {
            ResolutionPriority::High
        } else if attempt_count > 3 {
            ResolutionPriority::Low // Deprioritize after many failures
        } else if !risk_factors.is_empty() {
            ResolutionPriority::Medium
        } else {
            ResolutionPriority::Low // Default: no risk factors = low risk
        };

        // Require human approval for high-risk or repeated failures
        let requires_human_approval = priority == ResolutionPriority::Critical
            || attempt_count > 5
            || !security_concerns.is_empty();

        RiskAssessment {
            priority,
            risk_factors,
            security_concerns,
            compliance_requirements,
            requires_human_approval,
        }
    }

    /// Clear resolution history for a capability (e.g., after successful resolution)
    pub fn clear_resolution_history(&self, capability_id: &str) {
        let mut history = self.resolution_history.write().unwrap();
        history.remove(capability_id);
    }

    // ─────────────────────────────────────────────────────────────────────────────

    pub fn list_pending_capabilities(&self) -> Vec<String> {
        let queue = self.queue.lock().unwrap();
        queue
            .queue
            .iter()
            .map(|r| r.capability_id.clone())
            .collect()
    }

    pub async fn process_queue(&self) -> RuntimeResult<()> {
        Ok(())
    }

    async fn trigger_auto_resume_for_capability(&self, _capability_id: &str) -> RuntimeResult<()> {
        // Stub implementation
        Ok(())
    }

    async fn discover_network_catalogs(
        &self,
        _capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Stub implementation
        Ok(None)
    }

    fn infer_base_url(url: &str) -> String {
        // Stub implementation
        url.to_string()
    }

    async fn attempt_multi_capability_synthesis(
        &self,
        _capability_id: &str,
        _context: &str,
        _base_url: &str,
    ) -> RuntimeResult<Option<Vec<CapabilityManifest>>> {
        // Stub implementation
        Ok(None)
    }

    fn infer_provider_slug(_capability_id: &str, _base_url: &str) -> String {
        "unknown-provider".to_string()
    }

    fn env_var_name_for_slug(slug: &str) -> String {
        format!("{}_API_KEY", slug.to_uppercase().replace("-", "_"))
    }

    fn infer_primary_query_param(_slug: &str, _capability_id: &str, _base_url: &str) -> String {
        "q".to_string()
    }

    fn infer_secondary_query_param(_primary: &str) -> String {
        "query".to_string()
    }

    fn emit_missing_capability_audit(&self, _capability_id: &str) -> RuntimeResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::registry::CapabilityRegistry;
    use crate::synthesis::core::feature_flags::MissingCapabilityFeatureFlags;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_missing_capability_queue() {
        let mut queue = MissingCapabilityQueue::new();
        assert!(!queue.has_pending());

        let request = MissingCapabilityRequest {
            capability_id: "test.capability".to_string(),
            arguments: vec![],
            context: HashMap::new(),
            requested_at: std::time::SystemTime::now(),
            attempt_count: 0,
        };

        queue.enqueue(request.clone());
        assert!(queue.has_pending());

        let stats = queue.stats();
        assert_eq!(stats.pending_count, 1);
        assert_eq!(stats.in_progress_count, 0);

        let dequeued = queue.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().capability_id, "test.capability");

        let stats = queue.stats();
        assert_eq!(stats.pending_count, 0);
        assert_eq!(stats.in_progress_count, 1);
    }

    #[tokio::test]
    async fn test_missing_capability_resolver() {
        let registry = Arc::new(RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let checkpoint_archive = Arc::new(CheckpointArchive::new());
        // Use a testing feature configuration that enables runtime detection
        // so that the resolver will actually enqueue missing capabilities
        // during unit tests.
        let mut test_cfg = MissingCapabilityConfig::default();
        test_cfg.feature_flags = MissingCapabilityFeatureFlags::testing();

        let resolver = MissingCapabilityResolver::new(
            marketplace,
            checkpoint_archive,
            ResolverConfig::default(),
            test_cfg,
        );

        let mut context = HashMap::new();
        context.insert("plan_id".to_string(), "test_plan".to_string());
        context.insert("intent_id".to_string(), "test_intent".to_string());

        // Test handling missing capability
        let result =
            resolver.handle_missing_capability("missing.capability".to_string(), vec![], context);
        assert!(result.is_ok());

        let stats = resolver.get_stats();
        assert_eq!(stats.pending_count, 1);
    }

    #[tokio::test]
    async fn test_capability_id_normalization_on_resolution() {
        use crate::capability_marketplace::types::{
            CapabilityManifest, LocalCapability, ProviderType,
        };
        use rtfs::runtime::values::Value;

        let registry = Arc::new(RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let checkpoint_archive = Arc::new(CheckpointArchive::new());

        // Register a capability with a clean ID
        let cap_id = "synth.domain.generated.capability.v1".to_string();
        let manifest = CapabilityManifest {
            id: cap_id.clone(),
            name: "Test Generated Capability".to_string(),
            description: "Test manifest for normalization".to_string(),
            version: "1.0.0".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: Arc::new(|_args| Ok(Value::String("ok".to_string()))),
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata: std::collections::HashMap::new(),
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
        };
        marketplace
            .register_capability_manifest(manifest)
            .await
            .unwrap();

        let mut test_cfg = MissingCapabilityConfig::default();
        test_cfg.feature_flags = MissingCapabilityFeatureFlags::testing();
        let resolver = MissingCapabilityResolver::new(
            Arc::clone(&marketplace),
            checkpoint_archive,
            ResolverConfig {
                verbose_logging: true,
                ..ResolverConfig::default()
            },
            test_cfg,
        );

        // Create a request with trailing whitespace/newline and quotes
        let request = MissingCapabilityRequest {
            capability_id: format!("\"{}\n\"", cap_id),
            arguments: vec![],
            context: HashMap::new(),
            requested_at: std::time::SystemTime::now(),
            attempt_count: 0,
        };

        // The resolver should normalize and detect the already-registered capability
        let result = resolver.resolve_capability(&request).await.unwrap();
        match result {
            ResolutionResult::Resolved {
                capability_id,
                resolution_method,
                ..
            } => {
                assert_eq!(capability_id, cap_id);
                assert_eq!(resolution_method, "marketplace_found".to_string());
            }
            other => panic!("Expected Resolved, got: {:?}", other),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolutionEvent {
    pub capability_id: String,
    pub stage: &'static str,
    pub summary: String,
    pub detail: Option<String>,
}

pub trait ResolutionObserver: Send + Sync {
    fn on_event(&self, event: ResolutionEvent);
}
