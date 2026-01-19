//! CCOS Governance Kernel
//!
//! This module defines the Governance Kernel, the high-privilege, secure component
//! responsible for enforcing the system's `Constitution`. It acts as the mandatory
//! intermediary between the low-privilege Arbiter and the Orchestrator.
//!
//! The Kernel's primary responsibilities include:
//! - Validating proposed plans against the Constitution.
//! - Sanitizing intents and scaffolding plans for safety.
//! - Verifying capability attestations.
//! - Logging all decisions and actions to the Causal Chain.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;

use crate::cognitive_engine::DelegatingCognitiveEngine;

use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

use super::governance_judge::PlanJudge;
use super::intent_graph::IntentGraph;
use super::orchestrator::Orchestrator;
use super::types::Intent; // for delegation validation
use super::types::{ExecutionResult, Plan, PlanBody, StorableIntent};
use rtfs::runtime::error::RuntimeError;

/// Action to take when a rule matches
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RuleAction {
    Allow,
    Deny(String), // Reason
    RequireHumanApproval,
    RequireGuardianApproval,
}

/// Used by GovernanceKernel to gate external capability synthesis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SynthesisRisk {
    /// Low risk - can be auto-resolved
    Low,
    /// Medium risk - auto-resolve with monitoring
    Medium,
    /// High risk - requires human approval
    High,
    /// Critical risk - manual intervention required
    Critical,
}

/// Result of LLM prompt sanitization
#[derive(Debug, Clone, PartialEq)]
pub enum LlmPromptResult {
    /// Prompt is safe to execute
    Safe,
    /// Prompt requires human approval before execution
    RequiresApproval(String),
}

/// Risk assessment for capability synthesis authorization.
/// Mirrors the assessment logic from MissingCapabilityResolver but lives
/// in GovernanceKernel for proper governance gating.
#[derive(Debug, Clone)]
pub struct SynthesisRiskAssessment {
    /// Overall risk level
    pub risk: SynthesisRisk,
    /// Risk factors identified
    pub risk_factors: Vec<String>,
    /// Security concerns (admin, auth, credentials)
    pub security_concerns: Vec<String>,
    /// Compliance requirements (PCI-DSS, GDPR)
    pub compliance_requirements: Vec<String>,
    /// Human approval required based on risk + config
    pub requires_human_approval: bool,
}

impl SynthesisRiskAssessment {
    /// Assess synthesis risk for a capability based on its ID.
    /// Returns a risk assessment that can be used to gate synthesis.
    pub fn assess(capability_id: &str) -> Self {
        let mut risk_factors = Vec::new();
        let mut security_concerns = Vec::new();
        let mut compliance_requirements = Vec::new();

        let id_lower = capability_id.to_lowercase();

        // Administrative capabilities
        if id_lower.contains("admin") || id_lower.contains("root") || id_lower.contains("sudo") {
            risk_factors.push("Administrative capability detected".to_string());
            security_concerns.push("High privilege access required".to_string());
        }

        // Financial capabilities
        if id_lower.contains("payment")
            || id_lower.contains("financial")
            || id_lower.contains("billing")
        {
            risk_factors.push("Financial capability detected".to_string());
            compliance_requirements.push("PCI-DSS compliance required".to_string());
        }

        // Security-related capabilities
        if id_lower.contains("auth")
            || id_lower.contains("security")
            || id_lower.contains("credential")
        {
            risk_factors.push("Security-related capability".to_string());
            security_concerns.push("Authentication/authorization access".to_string());
        }

        // Data access capabilities
        if id_lower.contains("database")
            || id_lower.contains("storage")
            || id_lower.contains("delete")
        {
            risk_factors.push("Data access capability".to_string());
            compliance_requirements.push("Data protection compliance required".to_string());
        }

        // Personal data capabilities
        if id_lower.contains("pii") || id_lower.contains("personal") || id_lower.contains("gdpr") {
            risk_factors.push("Personal data handling".to_string());
            compliance_requirements.push("GDPR compliance required".to_string());
        }

        // Determine risk level
        let risk = if security_concerns.len() > 1 || compliance_requirements.len() > 1 {
            SynthesisRisk::Critical
        } else if !security_concerns.is_empty() || !compliance_requirements.is_empty() {
            SynthesisRisk::High
        } else if !risk_factors.is_empty() {
            SynthesisRisk::Medium
        } else {
            SynthesisRisk::Low
        };

        let requires_human_approval =
            risk == SynthesisRisk::Critical || risk == SynthesisRisk::High;

        Self {
            risk,
            risk_factors,
            security_concerns,
            compliance_requirements,
            requires_human_approval,
        }
    }
}

/// A rule in the Constitution
#[derive(Debug, Clone, Serialize)]
pub struct ConstitutionRule {
    pub id: String,
    pub description: String,
    pub match_pattern: String, // Glob-like pattern for capability ID
    pub action: RuleAction,
}

/// Represents the system's constitution, a set of human-authored rules.
// TODO: This should be loaded from a secure, signed configuration file.
#[derive(Debug, Serialize)]
pub struct Constitution {
    pub rules: Vec<ConstitutionRule>,
    /// Execution hint policy limits
    pub hint_policies: ExecutionHintPolicies,
    /// Semantic judge configuration
    pub semantic_judge_policy: SemanticJudgePolicy,
}

/// Policy for the semantic plan judge
#[derive(Debug, Clone, Serialize)]
pub struct SemanticJudgePolicy {
    /// Whether the semantic judge is enabled
    pub enabled: bool,
    /// Whether to fail open if the LLM is unavailable or fails
    pub fail_open: bool,
    /// Risk score threshold (0.0 to 1.0). Plans with risk > threshold are blocked.
    pub risk_threshold: f64,
}

impl Default for SemanticJudgePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            fail_open: true,
            risk_threshold: 0.7,
        }
    }
}

/// Policy limits for execution hints
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionHintPolicies {
    /// Maximum allowed retry attempts (prevents DoS via infinite retries)
    pub max_retries: u32,
    /// Maximum timeout multiplier (prevents excessive resource hold)
    pub max_timeout_multiplier: f64,
    /// Maximum absolute timeout in milliseconds
    pub max_absolute_timeout_ms: u64,
    /// Capability ID patterns that are allowed as fallbacks (glob patterns)
    pub allowed_fallback_patterns: Vec<String>,
    /// Whether fallback capabilities must be in approved list
    pub require_approved_fallbacks: bool,
}

impl Default for ExecutionHintPolicies {
    fn default() -> Self {
        Self {
            max_retries: 5,
            max_timeout_multiplier: 10.0,
            max_absolute_timeout_ms: 300_000, // 5 minutes max
            allowed_fallback_patterns: vec!["*".to_string()], // Allow all by default
            require_approved_fallbacks: false,
        }
    }
}

impl Default for Constitution {
    fn default() -> Self {
        // Default rules as defined in the spec
        let rules = vec![
            ConstitutionRule {
                id: "cli-agent-restrictions".to_string(),
                description: "Agents cannot modify system configuration without human approval"
                    .to_string(),
                match_pattern: "ccos.cli.config.*".to_string(),
                action: RuleAction::RequireHumanApproval,
            },
            ConstitutionRule {
                id: "cli-discovery-allowed".to_string(),
                description: "Agents can freely discover and search capabilities".to_string(),
                match_pattern: "ccos.cli.discovery.*".to_string(),
                action: RuleAction::Allow,
            },
            ConstitutionRule {
                id: "cli-approval-restricted".to_string(),
                description: "Only humans can approve new servers".to_string(),
                match_pattern: "ccos.cli.approval.approve".to_string(),
                action: RuleAction::RequireHumanApproval,
            },
            ConstitutionRule {
                id: "no-global-thermonuclear-war".to_string(),
                description: "Prevent global thermonuclear war".to_string(),
                match_pattern: "*launch-nukes*".to_string(),
                action: RuleAction::Deny("Rule against global thermonuclear war".to_string()),
            },
        ];

        Self {
            rules,
            hint_policies: ExecutionHintPolicies::default(),
            semantic_judge_policy: SemanticJudgePolicy::default(),
        }
    }
}

/// The Governance Kernel is the root of trust in the CCOS.
/// Its logic is designed to be simple, verifiable, and secure.
pub struct GovernanceKernel {
    orchestrator: Arc<Orchestrator>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    constitution: Constitution,
    /// Optional reference to the DelegatingCognitiveEngine for centralized LLM access
    delegating_arbiter: RwLock<Option<Arc<DelegatingCognitiveEngine>>>,
    plan_judge: PlanJudge,
}

impl GovernanceKernel {
    /// Creates a new Governance Kernel.
    pub fn new(orchestrator: Arc<Orchestrator>, intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            orchestrator,
            intent_graph,
            constitution: Constitution::default(),
            delegating_arbiter: RwLock::new(None),
            plan_judge: PlanJudge::new(),
        }
    }

    /// Set the DelegatingCognitiveEngine for centralized LLM access (called after construction)
    pub fn set_arbiter(&self, arbiter: Arc<DelegatingCognitiveEngine>) {
        if let Ok(mut guard) = self.delegating_arbiter.write() {
            *guard = Some(arbiter);
        }
    }

    /// Access the system Constitution.
    pub fn get_rules(&self) -> &Constitution {
        &self.constitution
    }

    /// Check authorization for synthesizing an external capability.
    ///
    /// This is called by the planner or MissingCapabilityResolver before
    /// synthesizing a capability from an external source (MCP, LLM, etc).
    ///
    /// Returns:
    /// - `RuleAction::Allow` - Proceed with synthesis
    /// - `RuleAction::RequireHumanApproval` - Queue for human approval
    /// - `RuleAction::Deny(reason)` - Block synthesis
    pub fn check_synthesis_authorization(&self, capability_id: &str) -> RuleAction {
        let assessment = SynthesisRiskAssessment::assess(capability_id);

        log::debug!(
            "[GovernanceKernel] Synthesis risk for '{}': {:?} (factors: {:?})",
            capability_id,
            assessment.risk,
            assessment.risk_factors
        );

        match assessment.risk {
            SynthesisRisk::Low => RuleAction::Allow,
            SynthesisRisk::Medium => {
                // Medium risk: allow but log for monitoring
                log::info!(
                    "[GovernanceKernel] Medium-risk synthesis allowed: {} (factors: {:?})",
                    capability_id,
                    assessment.risk_factors
                );
                RuleAction::Allow
            }
            SynthesisRisk::High => {
                if assessment.requires_human_approval {
                    log::warn!(
                        "[GovernanceKernel] High-risk synthesis requires human approval: {} (security: {:?}, compliance: {:?})",
                        capability_id,
                        assessment.security_concerns,
                        assessment.compliance_requirements
                    );
                    RuleAction::RequireHumanApproval
                } else {
                    RuleAction::Allow
                }
            }
            SynthesisRisk::Critical => {
                let reason = format!(
                    "Critical risk synthesis blocked: {} (security: {:?}, compliance: {:?})",
                    capability_id, assessment.security_concerns, assessment.compliance_requirements
                );
                log::error!("[GovernanceKernel] {}", reason);
                RuleAction::Deny(reason)
            }
        }
    }

    /// Validates that an RTFS expression is pure (no side effects).
    ///
    /// Pure expressions can only use:
    /// - RTFS stdlib functions (map, filter, group-by, get, etc.)
    /// - Pure capability calls (rtfs.*, pure.*, math.*, generated/*)
    ///
    /// Impure patterns are blocked:
    /// - External MCP calls (mcp.*)
    /// - HTTP calls (http.*)
    /// - File system operations (fs.*)
    /// - Database operations (db.*)
    /// - I/O operations (ccos.io.* except println)
    /// - User interaction (ccos.user.*)
    ///
    /// Returns Ok(()) if pure, Err with reason if impure.
    pub fn validate_purity(&self, rtfs_code: &str) -> RuntimeResult<()> {
        // Patterns that indicate impure operations
        // These are capability ID prefixes that have side effects
        const IMPURE_PREFIXES: &[&str] = &[
            "mcp.",          // External MCP server calls
            "http.",         // HTTP calls
            "fs.",           // File system operations
            "db.",           // Database operations
            "ccos.io.write", // I/O write operations
            "ccos.user.",    // User interaction
            "ccos.cli.",     // CLI operations
            "ccos.config.",  // Configuration mutations
        ];

        // Check for impure (call ...) patterns
        for prefix in IMPURE_PREFIXES {
            let pattern = format!("(call \"{}", prefix);
            if rtfs_code.contains(&pattern) {
                return Err(RuntimeError::Generic(format!(
                    "Purity violation: adapter contains impure operation with prefix '{}'",
                    prefix
                )));
            }
        }

        // Note: Pure patterns are implicitly allowed:
        // - No (call ...) at all (pure stdlib expressions)
        // - (call "rtfs.*) - RTFS stdlib wrappers
        // - (call "pure.*) - Explicitly pure capabilities
        // - (call "math.*) - Pure mathematical functions
        // - (call "generated/*) - Auto-generated pure capabilities
        // - (call "ccos.io.println") - Safe output for debugging
        // - (call "ccos.data.*) - Pure data transformations

        log::debug!("[GovernanceKernel] Purity validation passed for adapter");
        Ok(())
    }

    /// Sanitizes an LLM prompt for use by agents.
    ///
    /// This performs comprehensive checks to prevent prompt injection and misuse:
    /// 1. **Injection Detection** - Blocks common jailbreak patterns
    /// 2. **Scope Enforcement** - Ensures prompt relates to provided context
    /// 3. **Risk Assessment** - Flags high-risk prompts for approval
    ///
    /// Returns:
    /// - Ok(LlmPromptResult::Safe) - Prompt is safe to execute
    /// - Ok(LlmPromptResult::RequiresApproval(reason)) - Needs human approval
    /// - Err - Prompt is blocked (injection detected)
    pub fn sanitize_llm_prompt(
        &self,
        prompt: &str,
        context_size: usize,
    ) -> RuntimeResult<LlmPromptResult> {
        let prompt_lower = prompt.to_lowercase();

        // === 1. Injection Detection ===
        const INJECTION_PATTERNS: &[&str] = &[
            "ignore all previous instructions",
            "ignore previous instructions",
            "forget your instructions",
            "disregard your instructions",
            "disregard previous",
            "you are now",
            "pretend you are",
            "act as if you are",
            "roleplay as",
            "your new role is",
            "from now on you will",
            "system prompt",
            "jailbreak",
            "dan mode",
            "developer mode",
            "bypass",
            "override your",
            "ignore safety",
            "ignore your training",
        ];

        for pattern in INJECTION_PATTERNS {
            if prompt_lower.contains(pattern) {
                return Err(RuntimeError::Generic(format!(
                    "LLM prompt injection blocked: pattern '{}' detected",
                    pattern
                )));
            }
        }

        // === 2. Dangerous Request Detection ===
        const DANGEROUS_PATTERNS: &[&str] = &[
            "password",
            "api key",
            "secret key",
            "private key",
            "access token",
            "credentials",
            "ssh key",
            "execute code",
            "run command",
            "shell command",
            "rm -rf",
            "drop table",
            "delete from",
            "format c:",
        ];

        for pattern in DANGEROUS_PATTERNS {
            if prompt_lower.contains(pattern) {
                log::warn!(
                    "[GovernanceKernel] LLM prompt contains risky pattern: {}",
                    pattern
                );
                return Ok(LlmPromptResult::RequiresApproval(format!(
                    "Prompt mentions sensitive topic: '{}'",
                    pattern
                )));
            }
        }

        // === 3. Size-based Risk Assessment ===
        // Very long prompts without context are suspicious
        if prompt.len() > 2000 && context_size == 0 {
            return Ok(LlmPromptResult::RequiresApproval(
                "Long prompt without context data may indicate injection attempt".to_string(),
            ));
        }

        // === 4. Character-based Checks ===
        // Excessive special characters may indicate obfuscation
        let special_char_ratio = prompt
            .chars()
            .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
            .count() as f64
            / prompt.len() as f64;

        if special_char_ratio > 0.3 && prompt.len() > 100 {
            return Ok(LlmPromptResult::RequiresApproval(
                "High ratio of special characters detected".to_string(),
            ));
        }

        log::debug!("[GovernanceKernel] LLM prompt sanitization passed");
        Ok(LlmPromptResult::Safe)
    }

    /// Set the semantic judge policy.
    pub fn set_semantic_judge_policy(&mut self, policy: SemanticJudgePolicy) {
        self.constitution.semantic_judge_policy = policy;
    }

    /// Performs a semantic judgment of the plan using an LLM.
    /// This acts as a "common sense" check to ensure the plan aligns with the goal.
    pub async fn judge_plan_semantically(
        &self,
        plan: &Plan,
        intent: Option<&StorableIntent>,
    ) -> RuntimeResult<()> {
        let policy = &self.constitution.semantic_judge_policy;
        if !policy.enabled {
            ccos_eprintln!("   ⚖️  [SemanticJudge] Disabled - skipping judgment");
            return Ok(());
        }

        let arbiter_opt = {
            self.delegating_arbiter
                .read()
                .ok()
                .and_then(|guard| guard.clone())
        };

        let arbiter = match arbiter_opt {
            Some(a) => a,
            None => {
                if policy.fail_open {
                    ccos_eprintln!(
                        "   ⚖️  [SemanticJudge] No arbiter available - failing open (allowed)"
                    );
                    return Ok(());
                } else {
                    ccos_eprintln!(
                        "   🛑 [SemanticJudge] No arbiter available - failing closed (blocked)"
                    );
                    return Err(RuntimeError::Generic(
                        "Semantic judgment required but no arbiter available (fail-closed)"
                            .to_string(),
                    ));
                }
            }
        };

        let goal = intent
            .map(|i| i.goal.as_str())
            .or_else(|| plan.annotations.get("goal").and_then(|v| v.as_string()))
            .or_else(|| plan.metadata.get("goal").and_then(|v| v.as_string()))
            .unwrap_or("Unknown goal");

        // For now, we don't have a full resolution map in the Plan struct,
        // but the PlanJudge can still evaluate the RTFS code against the goal.
        let resolutions = HashMap::new();

        match self
            .plan_judge
            .judge_plan(arbiter.llm_provider(), goal, plan, &resolutions)
            .await
        {
            Ok(judgment) => {
                if judgment.allowed && judgment.risk_score <= policy.risk_threshold {
                    Ok(())
                } else {
                    let reason = if judgment.risk_score > policy.risk_threshold {
                        format!(
                            "Plan rejected by semantic judge: Risk score {:.2} exceeds threshold {:.2}. Reasoning: {}",
                            judgment.risk_score, policy.risk_threshold, judgment.reasoning
                        )
                    } else {
                        format!(
                            "Plan rejected by semantic judge: {} (Risk Score: {:.2})",
                            judgment.reasoning, judgment.risk_score
                        )
                    };
                    Err(RuntimeError::Generic(reason))
                }
            }
            Err(e) => {
                if policy.fail_open {
                    ccos_eprintln!(
                        "   ⚠️  [SemanticJudge] LLM judgment failed: {}. Failing open.",
                        e
                    );
                    Ok(())
                } else {
                    ccos_eprintln!(
                        "   🛑 [SemanticJudge] LLM judgment failed: {}. Failing closed.",
                        e
                    );
                    Err(RuntimeError::Generic(format!(
                        "Semantic judgment failed: {}",
                        e
                    )))
                }
            }
        }
    }

    /// The primary entry point for processing a plan from the Arbiter.
    /// It validates the plan and, if successful, passes it to the Orchestrator.
    pub async fn validate_and_execute(
        &self,
        plan: Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // --- 1. Intent Sanitization (SEP-012) ---
        // For capability-internal plans, intent may be None. Only sanitize if present.
        let intent_opt = self.get_intent(&plan)?;
        if let Some(ref intent) = intent_opt {
            self.sanitize_intent(intent, &plan)?;
        }

        // --- 2. Plan Scaffolding (SEP-012) ---
        let safe_plan = self.scaffold_plan(plan)?;

        // --- 3. Execution Mode Detection (Criticality-Based Execution) ---
        // Read execution mode from plan policies or intent constraints
        // We detect this early to use it in constitution validation
        let execution_mode = self.detect_execution_mode(&safe_plan, intent_opt.as_ref())?;

        // --- 4. Constitution Validation (SEP-010) ---
        // Pass execution mode to validation logic
        self.validate_against_constitution(&safe_plan, &execution_mode)?;

        // --- 5. Semantic Judgment (New Step) ---
        self.judge_plan_semantically(&safe_plan, intent_opt.as_ref())
            .await?;

        // --- 6. Execution Mode Validation ---
        // Validate execution mode is compatible with plan security requirements
        self.validate_execution_mode(&safe_plan, intent_opt.as_ref(), &execution_mode)?;

        // --- 7. Attestation Verification (SEP-011) ---
        // TODO: Verify the cryptographic attestations of all capabilities
        // called within the plan.

        // Store execution mode in context for RuntimeHost to access
        let mut context_with_mode = context.clone();
        context_with_mode.cross_plan_params.insert(
            "execution_mode".to_string(),
            Value::String(execution_mode.clone()),
        );

        // --- 8a. Pre-execution Parse Validation with LLM Repair ---
        // Validate RTFS syntax before execution, attempt LLM repair if parsing fails
        let mut plan_to_execute = safe_plan.clone();
        if let PlanBody::Rtfs(ref rtfs_code) = safe_plan.body {
            use rtfs::parser::parse_expression;
            if let Err(parse_err) = parse_expression(rtfs_code.trim()) {
                log::warn!(
                    "[GovernanceKernel] RTFS parse error detected, attempting LLM repair: {:?}",
                    parse_err
                );

                // Try LLM repair for parse errors
                use crate::config::ValidationConfig;
                use crate::synthesis::validation::llm_repair_runtime_error;
                let validation_config = ValidationConfig::default();

                if validation_config.enable_runtime_repair {
                    let error_msg = format!("RTFS parse error: {:?}", parse_err);
                    let mut current_code = rtfs_code.clone();

                    for attempt in 1..=validation_config.max_runtime_repair_attempts {
                        log::info!(
                            "[GovernanceKernel] LLM parse-error repair attempt {}/{}",
                            attempt,
                            validation_config.max_runtime_repair_attempts
                        );

                        match llm_repair_runtime_error(
                            &current_code,
                            &error_msg,
                            attempt,
                            &validation_config,
                        )
                        .await
                        {
                            Ok(Some(repaired_code)) => {
                                // Validate repaired code parses
                                if parse_expression(repaired_code.trim()).is_ok() {
                                    log::info!(
                                        "[GovernanceKernel] LLM parse repair succeeded on attempt {}",
                                        attempt
                                    );
                                    plan_to_execute.body = PlanBody::Rtfs(repaired_code);
                                    break;
                                } else {
                                    log::warn!(
                                        "[GovernanceKernel] LLM repair still has parse errors, continuing"
                                    );
                                    current_code = repaired_code;
                                }
                            }
                            Ok(None) => {
                                log::info!(
                                    "[GovernanceKernel] LLM parse repair returned no fix, stopping"
                                );
                                break;
                            }
                            Err(e) => {
                                log::warn!("[GovernanceKernel] LLM parse repair error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // --- 8b. Execution ---
        // If all checks pass, delegate execution to the Orchestrator.
        // Execution mode is passed via context cross_plan_params for RuntimeHost to use
        let result = self
            .orchestrator
            .execute_plan(&plan_to_execute, &context_with_mode)
            .await;

        // --- 8c. Reactive Auto-Repair (fast pattern-based, then LLM dialog) ---
        // On runtime errors, attempt fast pattern-based repair first, then LLM dialog
        if let Err(ref e) = result {
            let error_msg = e.to_string();

            if let PlanBody::Rtfs(ref rtfs_code) = safe_plan.body {
                // First try fast pattern-based repair
                if error_msg.contains("expected map")
                    || error_msg.contains("got vector with keyword")
                {
                    use crate::planner::modular_planner::repair_rules::{
                        attempt_repair, RepairContext, RepairResult,
                    };

                    let ctx = RepairContext {
                        error_message: error_msg.clone(),
                        failed_expression: rtfs_code.clone(),
                        schemas: std::collections::HashMap::new(),
                    };

                    if let RepairResult::Repaired(repaired_code) = attempt_repair(&ctx) {
                        log::info!(
                            "[GovernanceKernel] Reactive pattern repair applied, retrying execution"
                        );

                        // Create repaired plan
                        let mut repaired_plan = safe_plan.clone();
                        repaired_plan.body = PlanBody::Rtfs(repaired_code);

                        // Retry execution with repaired plan
                        return self
                            .orchestrator
                            .execute_plan(&repaired_plan, &context_with_mode)
                            .await;
                    }
                }

                // --- LLM Dialog Repair Loop ---
                // Uses DelegatingCognitiveEngine for centralized LLM access
                let arbiter_opt = {
                    self.delegating_arbiter
                        .read()
                        .ok()
                        .and_then(|guard| guard.clone())
                };

                if let Some(arbiter) = arbiter_opt {
                    let max_repair_attempts = 3;
                    let mut current_plan = rtfs_code.clone();
                    let mut last_error = error_msg.clone();

                    for attempt in 1..=max_repair_attempts {
                        ccos_eprintln!(
                            "🔧 [GovernanceKernel] LLM repair attempt {}/{}",
                            attempt,
                            max_repair_attempts
                        );

                        // Build repair prompt
                        let prompt = format!(
                            r#"You are fixing an RTFS plan that failed during execution.

Original plan:
```rtfs
{}
```

Runtime error:
{}

Repair attempt: {} of {}

Analyze the error and fix the plan. Common runtime issues:
- Type mismatch: Check parameter types (e.g., enum values like "OPEN" vs "all")
- `expected vector, got map`: Use (get x :key) to extract array field first
- `expected map, got vector`: Data is already a collection - use it directly
- `Undefined symbol`: Use only RTFS stdlib functions (map, filter, reduce, get, etc.)

RTFS stdlib includes: map, filter, reduce, first, rest, conj, get, assoc, count, empty?, nil?, +, -, *, /, =, not=, and, or, not, if, let, fn

Respond with ONLY the corrected RTFS plan code, no explanations."#,
                            current_plan, last_error, attempt, max_repair_attempts
                        );

                        match arbiter.generate_raw_text(&prompt).await {
                            Ok(response) => {
                                // Extract RTFS code from response
                                let repaired = Self::extract_rtfs_code(&response);

                                // Basic sanity check
                                if repaired.contains("(let")
                                    || repaired.contains("(call")
                                    || repaired.contains("(do")
                                {
                                    ccos_eprintln!(
                                        "🔧 [GovernanceKernel] LLM produced candidate fix"
                                    );

                                    // Create repaired plan
                                    let mut repaired_plan = safe_plan.clone();
                                    repaired_plan.body = PlanBody::Rtfs(repaired.clone());

                                    // Re-validate against constitution
                                    if let Err(e) = self.validate_against_constitution(
                                        &repaired_plan,
                                        &execution_mode,
                                    ) {
                                        ccos_eprintln!(
                                            "⚠️  [GovernanceKernel] Repaired plan failed constitution: {}",
                                            e
                                        );
                                        current_plan = repaired;
                                        last_error = e.to_string();
                                        continue;
                                    }

                                    // Try executing the repaired plan
                                    match self
                                        .orchestrator
                                        .execute_plan(&repaired_plan, &context_with_mode)
                                        .await
                                    {
                                        Ok(exec_result) => {
                                            ccos_eprintln!(
                                                "✅ [GovernanceKernel] LLM repair succeeded on attempt {}",
                                                attempt
                                            );
                                            ccos_eprintln!("📝 Repaired Plan:\n{}", repaired);
                                            return Ok(exec_result);
                                        }
                                        Err(exec_err) => {
                                            ccos_eprintln!(
                                                "⚠️  [GovernanceKernel] Repaired plan still failed: {}",
                                                exec_err
                                            );
                                            current_plan = repaired;
                                            last_error = exec_err.to_string();
                                        }
                                    }
                                } else {
                                    ccos_eprintln!("⚠️  [GovernanceKernel] LLM repair produced invalid response");
                                    break;
                                }
                            }
                            Err(e) => {
                                ccos_eprintln!("⚠️  [GovernanceKernel] LLM repair error: {}", e);
                                break;
                            }
                        }
                    }
                } else {
                    log::debug!("[GovernanceKernel] No arbiter available for LLM repair");
                }
            }
        }

        result
    }

    /// Extract RTFS code from LLM response (handles markdown code blocks)
    fn extract_rtfs_code(response: &str) -> String {
        // Check for markdown code block with rtfs tag
        if let Some(start) = response.find("```rtfs") {
            if let Some(end) = response[start + 7..].find("```") {
                return response[start + 7..start + 7 + end].trim().to_string();
            }
        }
        // Check for generic code block
        if let Some(start) = response.find("```") {
            if let Some(end) = response[start + 3..].find("```") {
                return response[start + 3..start + 3 + end].trim().to_string();
            }
            // If it starts with ``` but doesn't have closing, take rest as code
            return response[start + 3..].trim().to_string();
        }
        // Return as-is if no code block found
        response.trim().to_string()
    }

    /// Retrieves the primary intent associated with the plan, if present.
    /// Returns None for capability-internal plans that don't have associated intents.
    fn get_intent(&self, plan: &Plan) -> RuntimeResult<Option<StorableIntent>> {
        ccos_println!(
            "[GovernanceKernel] get_intent for plan: {:?} with intent_ids: {:?}",
            plan.plan_id,
            plan.intent_ids
        );
        let intent_id = match plan.intent_ids.first() {
            Some(id) => id,
            None => {
                ccos_println!("[GovernanceKernel] No intent IDs found in plan");
                return Ok(None);
            } // No intent for capability-internal plans
        };

        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        let intent = graph
            .get_intent(intent_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Intent not found: {}", intent_id)))?;

        ccos_println!(
            "[GovernanceKernel] Found intent: {} goal: {}",
            intent.intent_id,
            intent.goal
        );

        Ok(Some(intent))
    }

    /// Checks the plan and its originating intent for malicious patterns.
    fn sanitize_intent(&self, intent: &StorableIntent, plan: &Plan) -> RuntimeResult<()> {
        // Check for common prompt injection phrases in the original request.
        let lower_request = intent.original_request.to_lowercase();
        const INJECTION_PHRASES: &[&str] = &[
            "ignore all previous instructions",
            "you are now in developer mode",
        ];
        for phrase in INJECTION_PHRASES {
            if lower_request.contains(phrase) {
                return Err(RuntimeError::Generic(
                    "Potential prompt injection detected".to_string(),
                ));
            }
        }

        // Check for logical inconsistencies between the intent and the plan.
        // Example: If intent is to send an email, the plan shouldn't be deleting files.
        if intent.goal.contains("email") {
            if let PlanBody::Rtfs(body_text) = &plan.body {
                if body_text.contains("delete-file") {
                    return Err(RuntimeError::Generic(
                        "Plan action contradicts intent goal".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Wraps the plan's body in a safety harness.
    fn scaffold_plan(&self, mut plan: Plan) -> RuntimeResult<Plan> {
        // Extract the original body text
        let original_body = match &plan.body {
            PlanBody::Source(text) | PlanBody::Rtfs(text) => text.clone(),
            PlanBody::Binary(_) | PlanBody::Wasm(_) => {
                return Err(RuntimeError::Generic(
                    "Cannot scaffold binary plan body".to_string(),
                ))
            }
        };

        // Wrap the original body in a `(do ...)` block if it isn't already.
        let wrapped_body = if original_body.trim().starts_with("(") {
            original_body
        } else {
            format!("(do {})", original_body)
        };

        // NOTE: Previously we injected unimplemented forms like `(with-resource-limits ...)` and `(on-failure ...)`.
        // Those forms are not yet supported by the parser/runtime, causing execution failures.
        // For now, keep the plan safely wrapped with `do` only.
        plan.body = PlanBody::Rtfs(wrapped_body);
        Ok(plan)
    }

    /// Validates the plan against the rules of the system's Constitution.
    fn validate_against_constitution(
        &self,
        plan: &Plan,
        execution_mode: &str,
    ) -> RuntimeResult<()> {
        // Check required capabilities against rules
        for capability_id in &plan.capabilities_required {
            for rule in &self.constitution.rules {
                if self.matches_pattern(capability_id, &rule.match_pattern) {
                    match &rule.action {
                        RuleAction::Deny(reason) => {
                            return Err(RuntimeError::Generic(format!(
                                "Plan rejected by constitution rule '{}': {}",
                                rule.id, reason
                            )));
                        }
                        RuleAction::RequireHumanApproval | RuleAction::RequireGuardianApproval => {
                            // If rule requires approval, execution mode must support it
                            // Modes "require-approval", "safe-only" (if deemed unsafe), and "dry-run" are acceptable.
                            // "full" mode is rejected if approval is required.

                            if execution_mode == "full" {
                                return Err(RuntimeError::Generic(format!(
                                    "Plan requires human approval for capability '{}' (rule '{}'), but execution mode is 'full'. Use 'require-approval' mode.",
                                    capability_id, rule.id
                                )));
                            }
                        }
                        RuleAction::Allow => {
                            // Explicit allow, continue checking other rules/capabilities
                            // (In a more complex system, this might override a Deny, but here we process all applicable rules)
                        }
                    }
                }
            }
        }

        // Keep existing simple check as well
        if let PlanBody::Rtfs(body_text) = &plan.body {
            if body_text.contains("launch-nukes") {
                return Err(RuntimeError::Generic(
                    "Plan violates Constitution: Rule against global thermonuclear war."
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Validates execution hints from RTFS metadata against Constitution policies.
    ///
    /// Checks:
    /// - Retry hint: max_retries <= constitution.hint_policies.max_retries
    /// - Timeout hint: multiplier <= max_timeout_multiplier, absolute_ms <= max_absolute_timeout_ms
    /// - Fallback hint: capability matches allowed_fallback_patterns (if require_approved_fallbacks)
    pub fn validate_execution_hints(
        &self,
        hints: &std::collections::HashMap<String, Value>,
    ) -> RuntimeResult<()> {
        let policies = &self.constitution.hint_policies;

        // Validate retry hints
        if let Some(retry_value) = hints.get("runtime.learning.retry") {
            if let Some(max_retries) = self
                .extract_u32_from_map(retry_value, "max-retries")
                .or_else(|| self.extract_u32_from_map(retry_value, "max"))
            {
                if max_retries > policies.max_retries {
                    return Err(RuntimeError::Generic(format!(
                        "Execution hint violated: retry max-retries={} exceeds policy limit of {}",
                        max_retries, policies.max_retries
                    )));
                }
            }
        }

        // Validate timeout hints
        if let Some(timeout_value) = hints.get("runtime.learning.timeout") {
            if let Some(multiplier) = self.extract_f64_from_map(timeout_value, "multiplier") {
                if multiplier > policies.max_timeout_multiplier {
                    return Err(RuntimeError::Generic(format!(
                        "Execution hint violated: timeout multiplier={:.1} exceeds policy limit of {:.1}",
                        multiplier, policies.max_timeout_multiplier
                    )));
                }
            }
            if let Some(absolute_ms) = self.extract_u64_from_map(timeout_value, "absolute-ms") {
                if absolute_ms > policies.max_absolute_timeout_ms {
                    return Err(RuntimeError::Generic(format!(
                        "Execution hint violated: absolute timeout={}ms exceeds policy limit of {}ms",
                        absolute_ms, policies.max_absolute_timeout_ms
                    )));
                }
            }
        }

        // Validate fallback hints
        if policies.require_approved_fallbacks {
            if let Some(fallback_value) = hints.get("runtime.learning.fallback") {
                if let Some(capability_id) =
                    self.extract_string_from_map(fallback_value, "capability")
                {
                    let allowed = policies
                        .allowed_fallback_patterns
                        .iter()
                        .any(|pattern| self.matches_pattern(&capability_id, pattern));
                    if !allowed {
                        return Err(RuntimeError::Generic(format!(
                            "Execution hint violated: fallback capability '{}' not in allowed list",
                            capability_id
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Helper to extract u32 from a RTFS map value
    fn extract_u32_from_map(&self, value: &Value, key: &str) -> Option<u32> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Integer(i) => Some(*i as u32),
                        Value::Float(f) => Some(*f as u32),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// Helper to extract u64 from a RTFS map value
    fn extract_u64_from_map(&self, value: &Value, key: &str) -> Option<u64> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Integer(i) => Some(*i as u64),
                        Value::Float(f) => Some(*f as u64),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// Helper to extract f64 from a RTFS map value
    fn extract_f64_from_map(&self, value: &Value, key: &str) -> Option<f64> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Float(f) => Some(*f),
                        Value::Integer(i) => Some(*i as f64),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// Helper to extract String from a RTFS map value
    fn extract_string_from_map(&self, value: &Value, key: &str) -> Option<String> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// Helper to check if an ID matches a pattern
    /// Supports simpler patterns:
    /// - "exact.match"
    /// - "prefix.*"
    /// - "*suffix"
    /// - "*contains*"
    fn matches_pattern(&self, id: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if let Some(prefix) = pattern.strip_suffix('*') {
            if let Some(suffix) = prefix.strip_prefix('*') {
                // "*contains*"
                return id.contains(suffix);
            }
            // "prefix*"
            return id.starts_with(prefix);
        }

        if let Some(suffix) = pattern.strip_prefix('*') {
            // "*suffix"
            return id.ends_with(suffix);
        }

        id == pattern
    }

    /// Delegation validation hook (M4): governance pre-approval of agent selection.
    /// Extend with constitutional / policy checks (e.g., trust tier allowlist, cost ceilings, jurisdiction constraints).
    /// Return Err(...) to veto delegation (arbiter will fall back to LLM planning path).
    pub fn validate_delegation(
        &self,
        intent: &Intent,
        agent_id: &str,
        score: f64,
    ) -> RuntimeResult<()> {
        // Placeholder policy examples (expand as specs evolve):
        // 1. Reject extremely low scores (defense in depth even if arbiter threshold handles it).
        if score < 0.50 {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "Delegation rejected: score {:.2} below governance floor for agent {}",
                score, agent_id
            )));
        }
        // 2. Enforce simple constraint: if intent goal mentions "EU" ensure agent id does not contain "non_eu" (placeholder heuristic).
        let goal_lower = intent.goal.to_lowercase();
        if goal_lower.contains("eu") && agent_id.contains("non_eu") {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Delegation rejected: agent jurisdiction mismatch (EU constraint)".to_string(),
            ));
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Execution Mode Detection (Criticality-Based Execution)
    // ---------------------------------------------------------------------

    /// Detect execution mode from plan policies or intent constraints
    /// Precedence: Plan policy > Intent constraint > Default (full)
    ///
    /// Execution modes:
    /// - "full": Execute all actions (default)
    /// - "dry-run": Validate plan without executing critical actions
    /// - "safe-only": Execute only safe actions, pause for critical ones
    /// - "require-approval": Pause and request approval for each critical action
    pub fn detect_execution_mode(
        &self,
        plan: &Plan,
        intent: Option<&StorableIntent>,
    ) -> RuntimeResult<String> {
        // Check plan policies first (highest precedence)
        // Plan policies are HashMap<String, Value>
        if let Some(execution_mode_value) = plan.policies.get("execution_mode") {
            if let Value::String(mode) = execution_mode_value {
                return Ok(mode.clone());
            }
        }

        // Check intent constraints (second precedence)
        // StorableIntent constraints are HashMap<String, String> (RTFS source expressions)
        // For execution-mode, we expect a simple string value like ":dry-run" or "dry-run"
        if let Some(intent) = intent {
            if let Some(constraint_str) = intent.constraints.get("execution-mode") {
                // Parse RTFS keyword or string value
                let mode = constraint_str
                    .trim()
                    .trim_start_matches(':') // Remove RTFS keyword prefix if present
                    .trim_matches('"') // Remove quotes if present
                    .to_string();
                if !mode.is_empty() {
                    return Ok(mode);
                }
            }
        }

        // Default: full execution
        Ok("full".to_string())
    }

    /// Validate that execution mode is compatible with plan and intent security requirements
    fn validate_execution_mode(
        &self,
        plan: &Plan,
        intent: Option<&StorableIntent>,
        execution_mode: &str,
    ) -> RuntimeResult<()> {
        // Check if plan has critical capabilities that require special handling
        let has_critical_capabilities = plan
            .capabilities_required
            .iter()
            .any(|cap_id| self.detect_security_level(cap_id) == "critical");

        // If plan has critical capabilities but execution mode is "full",
        // warn but allow (user explicitly requested full execution)
        if has_critical_capabilities && execution_mode == "full" {
            // Log warning but don't block - user may have explicitly set this
            eprintln!(
                "⚠️ Plan contains critical capabilities but execution mode is 'full' - \
                 consider using 'dry-run' or 'require-approval' for safety"
            );
        }

        Ok(())
    }

    /// Detect security level for a capability based on ID patterns or manifest metadata
    /// Returns: "low", "medium", "high", or "critical"
    ///
    /// This implements pattern-based detection as fallback when capabilities
    /// don't declare security levels in their manifest metadata.
    pub fn detect_security_level(&self, capability_id: &str) -> String {
        let id_lower = capability_id.to_lowercase();

        // CLI capability patterns
        if id_lower.starts_with("ccos.cli.") {
            // Critical: system modification
            if id_lower.contains("config.init") || id_lower.contains("governance.constitution") {
                return "critical".to_string();
            }
            // High: destructive or trust-modifying
            if id_lower.contains("remove") || id_lower.contains("approve") {
                return "high".to_string();
            }
            // Medium: state-changing but safe
            if id_lower.contains("add") || id_lower.contains("reject") || id_lower.contains("call")
            {
                return "medium".to_string();
            }
            // Default: read-only CLI operations
            return "low".to_string();
        }

        // Critical operations: payments, billing, charges, transfers
        if id_lower.contains("payment")
            || id_lower.contains("billing")
            || id_lower.contains("charge")
            || id_lower.contains("transfer")
            || id_lower.contains("refund")
        {
            return "critical".to_string();
        }

        // Critical operations: deletions, removals, destructive operations
        if id_lower.contains("delete")
            || id_lower.contains("remove")
            || id_lower.contains("destroy")
            || id_lower.contains("drop")
            || id_lower.contains("truncate")
        {
            return "critical".to_string();
        }

        // High-risk operations: system-level changes
        if id_lower.contains("exec")
            || id_lower.contains("shell")
            || id_lower.contains("system")
            || id_lower.contains("admin")
            || id_lower.contains("root")
        {
            return "high".to_string();
        }

        // Moderate operations: writes, creates, updates
        if id_lower.contains("write")
            || id_lower.contains("create")
            || id_lower.contains("update")
            || id_lower.contains("modify")
            || id_lower.contains("edit")
        {
            return "medium".to_string();
        }

        // Default: read operations are safe
        "low".to_string()
    }

    /// Check if a capability requires approval based on execution mode and security level
    pub fn requires_approval(&self, capability_id: &str, execution_mode: &str) -> bool {
        let security_level = self.detect_security_level(capability_id);

        match execution_mode {
            "require-approval" => {
                // Require approval for medium, high, or critical operations
                security_level == "medium"
                    || security_level == "high"
                    || security_level == "critical"
            }
            "safe-only" => {
                // Require approval for high or critical operations
                security_level == "high" || security_level == "critical"
            }
            "dry-run" => {
                // No approval needed in dry-run (will be simulated)
                false
            }
            "full" => {
                // No approval needed in full execution mode
                false
            }
            _ => {
                // Unknown mode - default to requiring approval for critical operations
                security_level == "critical"
            }
        }
    }

    /// Check if a capability should be simulated in dry-run mode
    pub fn should_simulate_in_dry_run(&self, capability_id: &str, execution_mode: &str) -> bool {
        if execution_mode != "dry-run" {
            return false;
        }

        let security_level = self.detect_security_level(capability_id);
        // Simulate high and critical operations in dry-run
        security_level == "high" || security_level == "critical"
    }

    // ---------------------------------------------------------------------
    // Governance-Enforced Execution Interfaces
    // ---------------------------------------------------------------------

    /// Execute a plan through the governance pipeline.
    /// This is the primary interface for external code to execute plans safely.
    ///
    /// # Security
    /// This method ensures all plan execution goes through the GovernanceKernel,
    /// providing constitutional validation, intent sanitization, and proper audit trails.
    pub async fn execute_plan_governed(
        &self,
        plan: Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // Use the existing validate_and_execute method which handles all governance checks
        self.validate_and_execute(plan, context).await
    }

    /// Execute an entire intent graph through the governance pipeline.
    /// This orchestrates child intents and manages shared context while ensuring governance compliance.
    ///
    /// # Security
    /// This method ensures all plan execution within the intent graph goes through the GovernanceKernel.
    pub async fn execute_intent_graph_governed(
        &self,
        root_intent_id: &str,
        initial_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // First validate that the root intent exists and can be executed
        let intent_id = root_intent_id.to_string();

        // Get the root intent from the graph
        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        let _root_intent = graph.get_intent(&intent_id).ok_or_else(|| {
            RuntimeError::Generic(format!("Intent not found: {}", root_intent_id))
        })?;

        drop(graph);

        // Get all child intents for this root intent
        let children = {
            let graph = self
                .intent_graph
                .lock()
                .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;
            graph.get_child_intents(&intent_id)
        };

        // Execute each child intent through governance
        let mut enhanced_context = initial_context.clone();
        enhanced_context.cross_plan_params.clear();

        let mut child_results = Vec::new();
        for child_intent in children {
            if let Some(child_plan) = self.get_plan_for_intent(&child_intent.intent_id)? {
                // Execute each child plan through governance
                let child_result = self
                    .validate_and_execute(child_plan, &enhanced_context)
                    .await?;
                let exported = self.extract_exported_variables(&child_result);
                enhanced_context.cross_plan_params.extend(exported);
                child_results.push((child_intent.intent_id.clone(), child_result));
            }
        }

        // Execute root plan if it exists
        let mut root_result = None;
        if let Some(root_plan) = self.get_plan_for_intent(root_intent_id)? {
            root_result = Some(
                self.validate_and_execute(root_plan, &enhanced_context)
                    .await?,
            );
        }

        // Build result summary
        let mut result_summary = Vec::new();

        for (child_id, result) in &child_results {
            if result.success {
                result_summary.push(format!("{}: {}", child_id, result.value));
            } else {
                result_summary.push(format!("{}: failed", child_id));
            }
        }

        if let Some(ref root) = root_result {
            if root.success {
                result_summary.push(format!("root: {}", root.value));
            } else {
                result_summary.push("root: failed".to_string());
            }
        }

        if result_summary.is_empty() {
            Ok(ExecutionResult {
                success: false,
                value: rtfs::runtime::values::Value::String("No plans executed".to_string()),
                metadata: Default::default(),
            })
        } else {
            Ok(ExecutionResult {
                success: true,
                value: rtfs::runtime::values::Value::String(format!(
                    "Governed orchestration of {} plans: {}",
                    child_results.len(),
                    result_summary.join(", ")
                )),
                metadata: Default::default(),
            })
        }
    }

    /// Get plan for a specific intent (governance-aware lookup)
    fn get_plan_for_intent(&self, intent_id: &str) -> RuntimeResult<Option<Plan>> {
        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        // Get plans associated with this intent from the orchestrator's plan archive
        // Use the governance-accessible method
        let plan_archive = self.orchestrator.get_plan_archive();

        let archivable_plans = plan_archive.get_plans_for_intent(&intent_id.to_string());

        if let Some(archivable_plan) = archivable_plans.first() {
            Ok(Some(Self::archivable_plan_to_plan(archivable_plan)))
        } else {
            Ok(None)
        }
    }

    /// Helper function to convert ArchivablePlan to Plan (duplicated from orchestrator for governance use)
    fn archivable_plan_to_plan(archivable_plan: &super::archivable_types::ArchivablePlan) -> Plan {
        use rtfs::parser::parse_expression;
        use rtfs::runtime::evaluator::Evaluator;
        use rtfs::runtime::execution_outcome::ExecutionOutcome;
        use rtfs::runtime::module_runtime::ModuleRegistry;
        use rtfs::runtime::pure_host::create_pure_host;
        use rtfs::runtime::security::RuntimeContext;

        use serde_json::Value as JsonValue;

        // Helper function to parse RTFS or JSON strings
        fn deserialize_value(value_str: &str) -> Option<Value> {
            if let Ok(expr) = parse_expression(value_str) {
                let module_registry = ModuleRegistry::new();
                let security_context = RuntimeContext::pure();
                let host = create_pure_host();
                let evaluator = Evaluator::new(
                    std::sync::Arc::new(module_registry),
                    security_context,
                    host,
                    rtfs::compiler::expander::MacroExpander::default(),
                );

                match evaluator.evaluate(&expr) {
                    Ok(ExecutionOutcome::Complete(value)) => Some(value),
                    _ => serde_json::from_str::<JsonValue>(value_str)
                        .ok()
                        .map(convert_json_value),
                }
            } else {
                serde_json::from_str::<JsonValue>(value_str)
                    .ok()
                    .map(convert_json_value)
            }
        }

        fn convert_json_value(json_val: JsonValue) -> Value {
            match json_val {
                JsonValue::Null => Value::Nil,
                JsonValue::Bool(b) => Value::Boolean(b),
                JsonValue::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Value::Integer(i)
                    } else if let Some(f) = n.as_f64() {
                        Value::Float(f)
                    } else {
                        Value::String(n.to_string())
                    }
                }
                JsonValue::String(s) => Value::String(s),
                JsonValue::Array(arr) => {
                    let runtime_vec: Vec<Value> = arr.into_iter().map(convert_json_value).collect();
                    Value::Vector(runtime_vec)
                }
                JsonValue::Object(obj) => {
                    let mut runtime_map = std::collections::HashMap::new();
                    for (k, v) in obj {
                        runtime_map.insert(rtfs::ast::MapKey::String(k), convert_json_value(v));
                    }
                    Value::Map(runtime_map)
                }
            }
        }

        // Extract the plan body
        let raw_body = match &archivable_plan.body {
            crate::archivable_types::ArchivablePlanBody::String(s) => s.clone(),
            crate::archivable_types::ArchivablePlanBody::Legacy { steps, .. } => {
                steps.first().cloned().unwrap_or_else(|| "()".to_string())
            }
        };

        // If the body is a (plan ...) form, extract the :body property
        let plan_body = if raw_body.trim().starts_with("(plan") {
            match rtfs::parser::parse(&raw_body) {
                Ok(top_levels) => {
                    if let Some(rtfs::ast::TopLevel::Plan(plan_def)) = top_levels.first() {
                        if let Some(body_prop) =
                            plan_def.properties.iter().find(|p| p.key.0 == "body")
                        {
                            crate::rtfs_bridge::expression_to_rtfs_string(&body_prop.value)
                        } else {
                            raw_body
                        }
                    } else {
                        raw_body
                    }
                }
                Err(_) => raw_body,
            }
        } else {
            raw_body
        };

        // Convert ArchivablePlan back to Plan
        Plan {
            plan_id: archivable_plan.plan_id.clone(),
            name: archivable_plan.name.clone(),
            intent_ids: archivable_plan.intent_ids.clone(),
            language: super::types::PlanLanguage::Rtfs20,
            body: super::types::PlanBody::Rtfs(plan_body),
            status: archivable_plan.status.clone(),
            created_at: archivable_plan.created_at,
            metadata: archivable_plan
                .metadata
                .iter()
                .filter_map(|(k, v)| deserialize_value(v).map(|val| (k.clone(), val)))
                .collect(),
            input_schema: archivable_plan
                .input_schema
                .as_ref()
                .and_then(|s| deserialize_value(s)),
            output_schema: archivable_plan
                .output_schema
                .as_ref()
                .and_then(|s| deserialize_value(s)),
            policies: archivable_plan
                .policies
                .iter()
                .filter_map(|(k, v)| deserialize_value(v).map(|val| (k.clone(), val)))
                .collect(),
            capabilities_required: archivable_plan.capabilities_required.clone(),
            annotations: archivable_plan
                .annotations
                .iter()
                .filter_map(|(k, v)| deserialize_value(v).map(|val| (k.clone(), val)))
                .collect(),
        }
    }

    /// Extract exported variables from execution result (simplified version)
    fn extract_exported_variables(&self, result: &ExecutionResult) -> HashMap<String, Value> {
        let mut exported = HashMap::new();
        if result.success {
            exported.insert("result".to_string(), result.value.clone());
        }
        exported
    }
}
