//! Continuous Resolution Loop
//!
//! This module implements Phase 7 of the missing capability resolution plan:
//! - Auto-trigger resolution pipeline on runtime failures
//! - Backoff and persistence for unresolved items
//! - Human-in-the-loop pause for high-risk capabilities
//! - Repeatable resolution with safe fallbacks

use crate::capability_marketplace::CapabilityMarketplace;
use crate::mcp::registry::MCPRegistryClient;
use crate::synthesis::core::missing_capability_resolver::MissingCapabilityResolver;
use crate::synthesis::introspection::mcp_introspector::MCPIntrospector;
use crate::synthesis::registration::registration_flow::RegistrationFlow;
use chrono::{DateTime, Utc};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
/// Resolution attempt record
#[derive(Debug, Clone)]
pub struct ResolutionAttempt {
    /// Capability ID being resolved
    pub capability_id: String,
    /// Timestamp of the attempt
    pub attempted_at: DateTime<Utc>,
    /// Number of attempts so far
    pub attempt_count: u32,
    /// Resolution method used
    pub resolution_method: ResolutionMethod,
    /// Success status
    pub success: bool,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Next retry time (if failed)
    pub next_retry_at: Option<DateTime<Utc>>,
}

/// Resolution method used
#[derive(Debug, Clone)]
pub enum ResolutionMethod {
    /// MCP Registry discovery
    McpRegistry,
    /// OpenAPI import
    OpenApiImport,
    /// GraphQL import
    GraphQLImport,
    /// HTTP wrapper
    HttpWrapper,
    /// LLM synthesis
    LlmSynthesis,
    /// Web search discovery
    WebSearch,
    /// Manual resolution
    Manual,
}

/// Resolution priority based on risk assessment
#[derive(Debug, Clone, PartialEq)]
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

/// Risk assessment for a capability
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
    /// Approval deadline
    pub approval_deadline: Option<DateTime<Utc>>,
}

/// Continuous resolution loop orchestrator
pub struct ContinuousResolutionLoop {
    /// Missing capability resolver
    resolver: Arc<MissingCapabilityResolver>,
    /// Registration flow for new capabilities
    registration_flow: Arc<RegistrationFlow>,
    /// Marketplace for capability management
    marketplace: Arc<CapabilityMarketplace>,
    /// Resolution attempts history
    resolution_history: Arc<RwLock<HashMap<String, Vec<ResolutionAttempt>>>>,
    /// Configuration for the loop
    config: ResolutionConfig,
    /// Human approval queue
    human_approval_queue: Arc<RwLock<Vec<PendingApproval>>>,
}

/// Configuration for continuous resolution
#[derive(Debug, Clone)]
pub struct ResolutionConfig {
    /// Maximum retry attempts per capability
    pub max_retry_attempts: u32,
    /// Base backoff delay in seconds
    pub base_backoff_seconds: u64,
    /// Maximum backoff delay in seconds
    pub max_backoff_seconds: u64,
    /// Human-in-the-loop timeout in hours
    pub human_approval_timeout_hours: u64,
    /// Auto-resolution enabled
    pub auto_resolution_enabled: bool,
    /// High-risk auto-resolution enabled
    pub high_risk_auto_resolution: bool,
}

/// Pending human approval
#[derive(Debug, Clone)]
pub struct PendingApproval {
    /// Capability ID
    pub capability_id: String,
    /// Risk assessment
    pub risk_assessment: RiskAssessment,
    /// Request timestamp
    pub requested_at: DateTime<Utc>,
    /// Requesting user/context
    pub requested_by: String,
    /// Approval deadline
    pub deadline: DateTime<Utc>,
    /// Approval status
    pub status: ApprovalStatus,
}

/// Approval status
#[derive(Debug, Clone)]
pub enum ApprovalStatus {
    /// Pending approval
    Pending,
    /// Approved by human
    Approved(String), // approver name
    /// Rejected by human
    Rejected(String, String), // rejector name, reason
    /// Expired (timeout)
    Expired,
}

impl ContinuousResolutionLoop {
    /// Create a new continuous resolution loop
    pub fn new(
        resolver: Arc<MissingCapabilityResolver>,
        registration_flow: Arc<RegistrationFlow>,
        marketplace: Arc<CapabilityMarketplace>,
        config: ResolutionConfig,
    ) -> Self {
        Self {
            resolver,
            registration_flow,
            marketplace,
            resolution_history: Arc::new(RwLock::new(HashMap::new())),
            config,
            human_approval_queue: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Process pending resolutions (called manually or by scheduler)
    pub async fn process_pending_resolutions(&self) -> RuntimeResult<()> {
        // Process pending resolutions
        Self::process_resolution_queue(
            &self.resolver,
            &self.registration_flow,
            &self.marketplace,
            &self.resolution_history,
            &self.human_approval_queue,
            &self.config,
        )
        .await?;

        // Check for expired approvals
        Self::process_expired_approvals(&self.human_approval_queue).await?;

        Ok(())
    }

    /// Trigger resolution for a missing capability
    pub async fn trigger_resolution(
        &self,
        capability_id: &str,
        context: Option<&str>,
    ) -> RuntimeResult<()> {
        println!("🔍 Triggering resolution for capability: {}", capability_id);

        // Assess risk level
        let risk_assessment = self.assess_risk(capability_id, context).await?;

        // Check if human approval is required
        if risk_assessment.requires_human_approval && !self.config.high_risk_auto_resolution {
            self.request_human_approval(
                capability_id,
                &risk_assessment,
                context.unwrap_or("system"),
            )
            .await?;
            return Ok(());
        }

        // Proceed with automatic resolution
        self.attempt_resolution(capability_id, &risk_assessment)
            .await
    }

    /// Assess risk level for a capability
    /// Delegates core assessment to MissingCapabilityResolver and adds deadline for human approval
    async fn assess_risk(
        &self,
        capability_id: &str,
        _context: Option<&str>,
    ) -> RuntimeResult<RiskAssessment> {
        // Delegate core risk assessment to the resolver
        let core_assessment = self.resolver.assess_risk(capability_id);

        // Convert resolver priority to local priority type
        let priority = match core_assessment.priority {
            crate::synthesis::core::missing_capability_resolver::ResolutionPriority::Low => {
                ResolutionPriority::Low
            }
            crate::synthesis::core::missing_capability_resolver::ResolutionPriority::Medium => {
                ResolutionPriority::Medium
            }
            crate::synthesis::core::missing_capability_resolver::ResolutionPriority::High => {
                ResolutionPriority::High
            }
            crate::synthesis::core::missing_capability_resolver::ResolutionPriority::Critical => {
                ResolutionPriority::Critical
            }
        };

        // Augment with additional context-specific risk factors
        let mut risk_factors = core_assessment.risk_factors;
        let mut compliance_requirements = core_assessment.compliance_requirements;

        // Add domain-specific risk indicators not covered by resolver
        if capability_id.contains("payment") || capability_id.contains("financial") {
            risk_factors.push("Financial capability detected".to_string());
            compliance_requirements.push("PCI-DSS compliance required".to_string());
        }

        if capability_id.contains("database") || capability_id.contains("storage") {
            risk_factors.push("Data access capability".to_string());
            compliance_requirements.push("Data protection compliance required".to_string());
        }

        // Determine if human approval is required (using resolver's assessment + config)
        let requires_human_approval = core_assessment.requires_human_approval
            || (priority == ResolutionPriority::High && !self.config.high_risk_auto_resolution);

        Ok(RiskAssessment {
            priority,
            risk_factors,
            security_concerns: core_assessment.security_concerns,
            compliance_requirements,
            requires_human_approval,
            approval_deadline: if requires_human_approval {
                Some(
                    Utc::now()
                        + chrono::Duration::hours(self.config.human_approval_timeout_hours as i64),
                )
            } else {
                None
            },
        })
    }

    /// Request human approval for high-risk capability
    async fn request_human_approval(
        &self,
        capability_id: &str,
        risk_assessment: &RiskAssessment,
        requested_by: &str,
    ) -> RuntimeResult<()> {
        let pending_approval = PendingApproval {
            capability_id: capability_id.to_string(),
            risk_assessment: risk_assessment.clone(),
            requested_at: Utc::now(),
            requested_by: requested_by.to_string(),
            deadline: risk_assessment.approval_deadline.unwrap(),
            status: ApprovalStatus::Pending,
        };

        let mut queue = self.human_approval_queue.write().await;
        queue.push(pending_approval);

        println!(
            "🛑 Human approval required for high-risk capability: {}",
            capability_id
        );
        println!("   Risk level: {:?}", risk_assessment.priority);
        println!("   Risk factors: {:?}", risk_assessment.risk_factors);
        println!(
            "   Deadline: {}",
            risk_assessment.approval_deadline.unwrap()
        );

        Ok(())
    }

    /// Attempt to resolve a capability
    async fn attempt_resolution(
        &self,
        capability_id: &str,
        risk_assessment: &RiskAssessment,
    ) -> RuntimeResult<()> {
        // Get previous attempts
        let history = self.resolution_history.read().await;
        let attempts = history.get(capability_id).cloned().unwrap_or_default();
        drop(history);

        // Number of attempts already recorded for this capability
        let base_attempt_count = attempts.len() as u32;

        // Check if we've exceeded max retry attempts
        if base_attempt_count >= self.config.max_retry_attempts {
            println!(
                "❌ Max retry attempts exceeded for capability: {}",
                capability_id
            );
            return Err(RuntimeError::Generic(format!(
                "Max retry attempts ({}) exceeded for capability: {}",
                self.config.max_retry_attempts, capability_id
            )));
        }

        // Try different resolution methods in order of preference
        let methods = self.get_resolution_methods(risk_assessment);

        let mut last_attempt_number: u32 = base_attempt_count;
        for (idx, method) in methods.into_iter().enumerate() {
            // Compute the current attempt number including prior attempts and this method index
            let current_attempt = base_attempt_count + (idx as u32) + 1;

            // Respect max attempts across methods as well
            if current_attempt > self.config.max_retry_attempts {
                println!(
                    "⛔ Reached max retry attempts ({}), skipping remaining methods",
                    self.config.max_retry_attempts
                );
                break;
            }

            // Calculate backoff delay based on current attempt number
            let backoff_delay = self.calculate_backoff_delay(current_attempt);
            let attempt = ResolutionAttempt {
                capability_id: capability_id.to_string(),
                attempted_at: Utc::now(),
                attempt_count: current_attempt,
                resolution_method: method.clone(),
                success: false,
                error_message: None,
                next_retry_at: Some(Utc::now() + chrono::Duration::seconds(backoff_delay as i64)),
            };

            match self.try_resolution_method(capability_id, &method).await {
                Ok(()) => {
                    println!(
                        "✅ Successfully resolved capability: {} using {:?}",
                        capability_id, method
                    );

                    // Record successful attempt in local history (with timestamps)
                    let mut history = self.resolution_history.write().await;
                    let attempts = history
                        .entry(capability_id.to_string())
                        .or_insert_with(Vec::new);
                    let mut successful_attempt = attempt.clone();
                    successful_attempt.success = true;
                    attempts.push(successful_attempt);

                    // Also record in resolver for persistence across restarts
                    self.resolver.record_resolution_attempt(
                        crate::synthesis::core::missing_capability_resolver::ResolutionAttempt {
                            capability_id: capability_id.to_string(),
                            attempted_at: std::time::SystemTime::now(),
                            attempt_count: current_attempt,
                            strategy_name: format!("{:?}", method),
                            success: true,
                            error_message: None,
                            next_retry_at: None,
                        },
                    );

                    // Clear resolver history on success (capability is now resolved)
                    self.resolver.clear_resolution_history(capability_id);

                    return Ok(());
                }
                Err(e) => {
                    println!(
                        "❌ Failed to resolve capability: {} using {:?}: {}",
                        capability_id, method, e
                    );

                    // Record failed attempt in local history
                    let mut history = self.resolution_history.write().await;
                    let attempts = history
                        .entry(capability_id.to_string())
                        .or_insert_with(Vec::new);
                    let mut failed_attempt = attempt;
                    failed_attempt.error_message = Some(e.to_string());
                    attempts.push(failed_attempt);

                    // Also record in resolver for persistence
                    let backoff_secs = self.calculate_backoff_delay(current_attempt);
                    self.resolver.record_resolution_attempt(
                        crate::synthesis::core::missing_capability_resolver::ResolutionAttempt {
                            capability_id: capability_id.to_string(),
                            attempted_at: std::time::SystemTime::now(),
                            attempt_count: current_attempt,
                            strategy_name: format!("{:?}", method),
                            success: false,
                            error_message: Some(e.to_string()),
                            next_retry_at: Some(
                                std::time::SystemTime::now()
                                    + std::time::Duration::from_secs(backoff_secs),
                            ),
                        },
                    );
                }
            }

            last_attempt_number = current_attempt;
        }

        // All methods failed, schedule retry if under limit
        if last_attempt_number < self.config.max_retry_attempts {
            println!(
                "⏳ Scheduling retry for capability: {} (attempt {})",
                capability_id, last_attempt_number
            );
        } else {
            println!(
                "💀 All resolution methods exhausted for capability: {}",
                capability_id
            );
        }

        Ok(())
    }

    /// Try a specific resolution method
    async fn try_resolution_method(
        &self,
        capability_id: &str,
        method: &ResolutionMethod,
    ) -> RuntimeResult<()> {
        match method {
            ResolutionMethod::McpRegistry => {
                // 1. Check overrides first
                let overrides = self.resolver.load_curated_overrides_for(capability_id)?;

                let server_url = if let Some(server) = overrides.first() {
                    println!("✅ Found override for capability: {}", capability_id);
                    if let Some(remotes) = &server.remotes {
                        MCPRegistryClient::select_best_remote_url(remotes)
                    } else {
                        None
                    }
                } else {
                    // TODO: Implement public registry search here
                    println!("⚠️ No override found for {}, and public registry search is not yet implemented.", capability_id);
                    None
                };

                if let Some(url) = server_url {
                    println!("🔍 Introspecting MCP server at {}", url);
                    let introspector = MCPIntrospector::new();

                    let mut headers = HashMap::new();
                    if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
                        headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                    }
                    let headers_opt = if headers.is_empty() {
                        None
                    } else {
                        Some(headers)
                    };

                    let server_name = "mcp-server";

                    let introspection = introspector
                        .introspect_mcp_server_with_auth(&url, server_name, headers_opt.clone())
                        .await?;

                    let manifests = introspector.create_capabilities_from_mcp(&introspection)?;

                    // Find the specific tool we are looking for
                    let tool_name = capability_id.split('.').last().unwrap_or(capability_id);

                    let mut manifest = manifests
                        .into_iter()
                        .find(|m| {
                            m.metadata
                                .get("mcp_tool_name")
                                .map(|name| name == tool_name)
                                .unwrap_or(false)
                        })
                        .ok_or_else(|| {
                            RuntimeError::Generic(format!(
                                "Tool '{}' not found in MCP server",
                                tool_name
                            ))
                        })?;

                    // Force the capability ID to match the requested ID
                    // This ensures the file is saved in the expected location (e.g. mcp/github/list_issues.rtfs)
                    manifest.id = capability_id.to_string();

                    // Load safe inputs for introspection from config/mcp_introspection.toml
                    let config_path = std::path::Path::new("config/mcp_introspection.toml");
                    let input_overrides = if config_path.exists() {
                        let content = std::fs::read_to_string(config_path).unwrap_or_default();
                        let config: toml::Value = toml::from_str(&content)
                            .unwrap_or(toml::Value::Table(toml::map::Map::new()));

                        config.get("tools").and_then(|t| t.get(tool_name)).map(|v| {
                            let json_str = serde_json::to_string(v).unwrap_or("{}".to_string());
                            serde_json::from_str(&json_str).unwrap_or(serde_json::Value::Null)
                        })
                    } else {
                        None
                    };

                    // Introspect output schema
                    if let Some(tool) = introspection
                        .tools
                        .iter()
                        .find(|t| t.tool_name == tool_name)
                    {
                        if let Ok((schema_opt, _)) = introspector
                            .introspect_output_schema(
                                tool,
                                &url,
                                server_name,
                                headers_opt,
                                input_overrides,
                            )
                            .await
                        {
                            if let Some(schema) = schema_opt {
                                manifest.output_schema = Some(schema);
                                println!("✅ Updated output schema for {}", tool_name);
                            }
                        }
                    }

                    // Generate implementation
                    let implementation =
                        introspector.generate_mcp_wrapper_code(&url, tool_name, &manifest.name);

                    // Save to disk - use workspace root
                    let output_dir =
                        crate::utils::fs::get_workspace_root().join("capabilities");
                    std::fs::create_dir_all(output_dir.join("mcp")).ok();

                    let path = introspector.save_capability_to_rtfs(
                        &manifest,
                        &implementation,
                        &output_dir,
                        None,
                    )?;

                    println!("💾 Saved capability to {:?}", path);

                    // Register in marketplace
                    self.registration_flow
                        .register_capability(manifest, Some(&implementation))
                        .await?;

                    Ok(())
                } else {
                    Err(RuntimeError::Generic(
                        "Could not determine MCP server URL from overrides or registry".to_string(),
                    ))
                }
            }

            ResolutionMethod::OpenApiImport => {
                // Try OpenAPI import (placeholder - would need actual implementation)
                Err(RuntimeError::Generic(
                    "OpenAPI import not yet implemented".to_string(),
                ))
            }

            ResolutionMethod::GraphQLImport => {
                // Try GraphQL import (placeholder - would need actual implementation)
                Err(RuntimeError::Generic(
                    "GraphQL import not yet implemented".to_string(),
                ))
            }

            ResolutionMethod::HttpWrapper => {
                // Try HTTP wrapper (placeholder - would need actual implementation)
                Err(RuntimeError::Generic(
                    "HTTP wrapper not yet implemented".to_string(),
                ))
            }

            ResolutionMethod::LlmSynthesis => {
                // Try LLM synthesis (placeholder - would need actual implementation)
                Err(RuntimeError::Generic(
                    "LLM synthesis not yet implemented".to_string(),
                ))
            }

            ResolutionMethod::WebSearch => {
                // Try web search discovery (placeholder - would need actual implementation)
                Err(RuntimeError::Generic(
                    "Web search discovery not yet implemented".to_string(),
                ))
            }

            ResolutionMethod::Manual => {
                // Reconstruct a minimal request context to reuse the existing strategy logic
                // In a real implementation, we might want to store/retrieve the original context
                let request =
                    crate::synthesis::missing_capability_resolver::MissingCapabilityRequest {
                        capability_id: capability_id.to_string(),
                        arguments: vec![], // No arguments available in this context
                        context: Default::default(),
                        requested_at: std::time::SystemTime::now(),
                        attempt_count: 1, // Logic handles attempts separately
                    };

                if let Some(manifest) = self
                    .resolver
                    .attempt_user_interaction(&request, capability_id)
                    .await?
                {
                    // Register if successful
                    self.marketplace
                        .register_capability_manifest(manifest)
                        .await?;
                    Ok(())
                } else {
                    Err(RuntimeError::Generic(
                        "User interaction did not resolve capability".to_string(),
                    ))
                }
            }
        }
    }

    /// Get resolution methods in order of preference based on risk assessment
    fn get_resolution_methods(&self, risk_assessment: &RiskAssessment) -> Vec<ResolutionMethod> {
        match risk_assessment.priority {
            ResolutionPriority::Low => {
                vec![
                    ResolutionMethod::McpRegistry,
                    ResolutionMethod::OpenApiImport,
                    ResolutionMethod::GraphQLImport,
                    ResolutionMethod::HttpWrapper,
                    ResolutionMethod::LlmSynthesis,
                    ResolutionMethod::WebSearch,
                    ResolutionMethod::Manual, // Added Manual to Low priority for demo/fallback
                ]
            }
            ResolutionPriority::Medium => {
                vec![
                    ResolutionMethod::McpRegistry,
                    ResolutionMethod::OpenApiImport,
                    ResolutionMethod::GraphQLImport,
                    ResolutionMethod::Manual,
                ]
            }
            ResolutionPriority::High | ResolutionPriority::Critical => {
                vec![ResolutionMethod::Manual]
            }
        }
    }

    /// Calculate backoff delay based on attempt count
    /// Delegates to MissingCapabilityResolver for unified backoff logic
    fn calculate_backoff_delay(&self, attempt_count: u32) -> u64 {
        self.resolver.calculate_backoff_delay(attempt_count)
    }

    /// Process the resolution queue
    async fn process_resolution_queue(
        resolver: &Arc<MissingCapabilityResolver>,
        registration_flow: &Arc<RegistrationFlow>,
        marketplace: &Arc<CapabilityMarketplace>,
        resolution_history: &Arc<RwLock<HashMap<String, Vec<ResolutionAttempt>>>>,
        human_approval_queue: &Arc<RwLock<Vec<PendingApproval>>>,
        config: &ResolutionConfig,
    ) -> RuntimeResult<()> {
        // Pull pending capabilities from the resolver queue
        let pending_capabilities: Vec<String> = resolver.list_pending_capabilities();

        for capability_id in pending_capabilities {
            // Check if we're still trying to resolve this capability
            let history = resolution_history.read().await;
            let attempts = history.get(&capability_id).cloned().unwrap_or_default();
            drop(history);

            let attempt_count = attempts.len() as u32;

            // Skip if we've exceeded max attempts
            if attempt_count >= config.max_retry_attempts {
                continue;
            }

            // Check if there's a pending human approval
            let approval_queue = human_approval_queue.read().await;
            let has_pending_approval = approval_queue.iter().any(|approval| {
                approval.capability_id == capability_id
                    && matches!(approval.status, ApprovalStatus::Pending)
            });
            drop(approval_queue);

            if has_pending_approval {
                continue;
            }

            // Check if it's time for the next retry
            if let Some(last_attempt) = attempts.last() {
                if let Some(next_retry_at) = last_attempt.next_retry_at {
                    if Utc::now() < next_retry_at {
                        continue;
                    }
                }
            }

            // Create a temporary instance to trigger resolution
            let loop_instance = ContinuousResolutionLoop::new(
                Arc::clone(resolver),
                Arc::clone(registration_flow),
                Arc::clone(marketplace),
                config.clone(),
            );

            if let Err(e) = loop_instance
                .trigger_resolution(&capability_id, Some("continuous_loop"))
                .await
            {
                eprintln!(
                    "⚠️ Failed to trigger resolution for {}: {}",
                    capability_id, e
                );
            }
        }

        Ok(())
    }

    /// Process expired approvals
    async fn process_expired_approvals(
        human_approval_queue: &Arc<RwLock<Vec<PendingApproval>>>,
    ) -> RuntimeResult<()> {
        let mut queue = human_approval_queue.write().await;
        let now = Utc::now();

        for approval in queue.iter_mut() {
            if matches!(approval.status, ApprovalStatus::Pending) && now > approval.deadline {
                approval.status = ApprovalStatus::Expired;
                println!(
                    "⏰ Approval expired for capability: {}",
                    approval.capability_id
                );
            }
        }

        // Remove expired approvals
        queue.retain(|approval| !matches!(approval.status, ApprovalStatus::Expired));

        Ok(())
    }

    /// Get pending capabilities from resolver (placeholder)
    async fn get_pending_capabilities(&self) -> RuntimeResult<Vec<String>> {
        // This would integrate with the actual resolver's pending queue
        // For now, return empty list
        Ok(vec![])
    }

    /// Approve a high-risk capability
    pub async fn approve_capability(
        &self,
        capability_id: &str,
        approver: &str,
    ) -> RuntimeResult<()> {
        let mut queue = self.human_approval_queue.write().await;

        for approval in queue.iter_mut() {
            if approval.capability_id == capability_id
                && matches!(approval.status, ApprovalStatus::Pending)
            {
                let risk_assessment = approval.risk_assessment.clone();
                approval.status = ApprovalStatus::Approved(approver.to_string());
                println!("✅ Capability {} approved by {}", capability_id, approver);

                // Trigger resolution now that it's approved
                drop(queue);
                return self
                    .attempt_resolution(capability_id, &risk_assessment)
                    .await;
            }
        }

        Err(RuntimeError::Generic(format!(
            "No pending approval found for capability: {}",
            capability_id
        )))
    }

    /// Reject a high-risk capability
    pub async fn reject_capability(
        &self,
        capability_id: &str,
        rejector: &str,
        reason: &str,
    ) -> RuntimeResult<()> {
        let mut queue = self.human_approval_queue.write().await;

        for approval in queue.iter_mut() {
            if approval.capability_id == capability_id
                && matches!(approval.status, ApprovalStatus::Pending)
            {
                approval.status =
                    ApprovalStatus::Rejected(rejector.to_string(), reason.to_string());
                println!(
                    "❌ Capability {} rejected by {}: {}",
                    capability_id, rejector, reason
                );
                return Ok(());
            }
        }

        Err(RuntimeError::Generic(format!(
            "No pending approval found for capability: {}",
            capability_id
        )))
    }

    /// Get resolution statistics
    pub async fn get_resolution_stats(&self) -> RuntimeResult<ResolutionStats> {
        let history = self.resolution_history.read().await;
        let approval_queue = self.human_approval_queue.read().await;

        let total_capabilities = history.len();
        let mut successful_resolutions = 0;
        let mut failed_resolutions = 0;
        let mut pending_approvals = 0;

        for attempts in history.values() {
            if let Some(last_attempt) = attempts.last() {
                if last_attempt.success {
                    successful_resolutions += 1;
                } else {
                    failed_resolutions += 1;
                }
            }
        }

        for approval in approval_queue.iter() {
            if matches!(approval.status, ApprovalStatus::Pending) {
                pending_approvals += 1;
            }
        }

        Ok(ResolutionStats {
            total_capabilities,
            successful_resolutions,
            failed_resolutions,
            pending_approvals,
            resolution_success_rate: if total_capabilities > 0 {
                successful_resolutions as f64 / total_capabilities as f64
            } else {
                0.0
            },
        })
    }
}

/// Resolution statistics
#[derive(Debug, Clone)]
pub struct ResolutionStats {
    pub total_capabilities: usize,
    pub successful_resolutions: usize,
    pub failed_resolutions: usize,
    pub pending_approvals: usize,
    pub resolution_success_rate: f64,
}

impl Default for ResolutionConfig {
    fn default() -> Self {
        Self {
            max_retry_attempts: 5,
            base_backoff_seconds: 30,
            max_backoff_seconds: 3600, // 1 hour
            human_approval_timeout_hours: 24,
            auto_resolution_enabled: true,
            high_risk_auto_resolution: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint_archive::CheckpointArchive;
    use crate::synthesis::core::missing_capability_resolver::MissingCapabilityResolver;

    #[tokio::test]
    async fn test_continuous_resolution_loop() {
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let checkpoint_archive = Arc::new(CheckpointArchive::new());
        let resolver = Arc::new(MissingCapabilityResolver::new(
            Arc::clone(&marketplace),
            Arc::clone(&checkpoint_archive),
            crate::synthesis::missing_capability_resolver::ResolverConfig::default(),
            crate::synthesis::feature_flags::MissingCapabilityConfig::default(),
        ));
        let registration_flow = Arc::new(RegistrationFlow::new(Arc::clone(&marketplace)));
        let config = ResolutionConfig::default();

        let loop_instance =
            ContinuousResolutionLoop::new(resolver, registration_flow, marketplace, config);

        // Test risk assessment
        let risk_assessment = loop_instance
            .assess_risk("test.capability", None)
            .await
            .unwrap();
        assert_eq!(risk_assessment.priority, ResolutionPriority::Low);
        assert!(!risk_assessment.requires_human_approval);

        let high_risk_assessment = loop_instance
            .assess_risk("admin.security.auth", None)
            .await
            .unwrap();
        assert_eq!(high_risk_assessment.priority, ResolutionPriority::Critical);
        assert!(high_risk_assessment.requires_human_approval);
    }

    #[test]
    fn test_backoff_calculation() {
        let config = ResolutionConfig::default();
        let loop_instance = ContinuousResolutionLoop::new(
            Arc::new(MissingCapabilityResolver::new(
                Arc::new(CapabilityMarketplace::new(Arc::new(
                    tokio::sync::RwLock::new(
                        crate::capabilities::registry::CapabilityRegistry::new(),
                    ),
                ))),
                Arc::new(CheckpointArchive::new()),
                crate::synthesis::missing_capability_resolver::ResolverConfig::default(),
                crate::synthesis::feature_flags::MissingCapabilityConfig::default(),
            )),
            Arc::new(RegistrationFlow::new(Arc::new(CapabilityMarketplace::new(
                Arc::new(tokio::sync::RwLock::new(
                    crate::capabilities::registry::CapabilityRegistry::new(),
                )),
            )))),
            Arc::new(CapabilityMarketplace::new(Arc::new(
                tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
            ))),
            config.clone(),
        );

        // Now delegates to resolver, which uses base_backoff_seconds: 60
        // Formula: base * 2^(attempt_count - 1), min 1 at attempt 0 → base * 1 = 60
        assert_eq!(loop_instance.calculate_backoff_delay(0), 60);
        assert_eq!(loop_instance.calculate_backoff_delay(1), 60);
        assert_eq!(loop_instance.calculate_backoff_delay(2), 120);
        assert_eq!(loop_instance.calculate_backoff_delay(10), 3600); // Max backoff
    }

    #[test]
    fn test_resolution_methods_priority() {
        let config = ResolutionConfig::default();
        let registry = Arc::new(tokio::sync::RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
        let loop_instance = ContinuousResolutionLoop::new(
            Arc::new(MissingCapabilityResolver::new(
                marketplace.clone(),
                Arc::new(CheckpointArchive::new()),
                crate::synthesis::missing_capability_resolver::ResolverConfig::default(),
                crate::synthesis::feature_flags::MissingCapabilityConfig::default(),
            )),
            Arc::new(RegistrationFlow::new(marketplace.clone())),
            marketplace,
            config,
        );

        let low_risk = RiskAssessment {
            priority: ResolutionPriority::Low,
            risk_factors: vec![],
            security_concerns: vec![],
            compliance_requirements: vec![],
            requires_human_approval: false,
            approval_deadline: None,
        };

        let methods = loop_instance.get_resolution_methods(&low_risk);
        assert_eq!(methods.len(), 7); // McpRegistry, OpenApiImport, GraphQLImport, HttpWrapper, LlmSynthesis, WebSearch, Manual
        assert!(matches!(methods[0], ResolutionMethod::McpRegistry));

        let critical_risk = RiskAssessment {
            priority: ResolutionPriority::Critical,
            risk_factors: vec!["High risk".to_string()],
            security_concerns: vec!["Security concern".to_string()],
            compliance_requirements: vec!["Compliance required".to_string()],
            requires_human_approval: true,
            approval_deadline: Some(Utc::now()),
        };

        let methods = loop_instance.get_resolution_methods(&critical_risk);
        assert_eq!(methods.len(), 1);
        assert!(matches!(methods[0], ResolutionMethod::Manual));
    }
}
