//! Modular Planner Demo
#![allow(dead_code, unused_imports)]
//!
//! This example demonstrates the new modular planning architecture that:
//! 1. Uses pluggable decomposition strategies (pattern-first, then LLM fallback)
//! 2. Properly stores all intents in the IntentGraph as real nodes
//! 3. Uses resolution strategies to map semantic intents to capabilities
//! 4. Generates executable RTFS plans from resolved capabilities
//! 5. EXECUTES the generated plan using the CCOS runtime
//!
//! The key difference from autonomous_agent_demo is that this architecture:
//! - Separates WHAT (decomposition produces semantic intents) from HOW (resolution finds capabilities)
//! - Uses pattern matching first for common goal structures (fast, deterministic)
//! - Falls back to LLM only when patterns don't match
//! - Stores all planning decisions in IntentGraph for audit/reuse
//!
//! Usage:
//!   cargo run --example modular_planner_demo -- --goal "list issues in mandubian/ccos but ask me for the page size"

use ccos::approval::{
    storage_file::FileApprovalStorage, storage_memory::InMemoryApprovalStorage, ApprovalAuthority,
    ApprovalCategory, RiskAssessment, RiskLevel, UnifiedApprovalQueue,
};
use ccos::examples_common::builder::ModularPlannerBuilder;
use ccos::planner::modular_planner::resolution::semantic::{CapabilityCatalog, CapabilityInfo};
use ccos::planner::modular_planner::{
    orchestrator::{PlanResult, TraceEvent},
    CatalogResolution, ModularPlanner, PatternDecomposition, ResolvedCapability,
};
use ccos::synthesis::dialogue::capability_synthesizer::{CapabilitySynthesizer, SynthesisRequest};
use clap::Parser;
use rtfs::runtime::security::RuntimeContext;
use std::error::Error;
use std::io::{self, Write};
use std::sync::Arc;

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
struct Args {
    /// Natural language goal
    #[arg(
        long,
        default_value = "list issues in mandubian/ccos but ask me for the page size"
    )]
    goal: String,

    /// Show detailed planning trace
    #[arg(long)]
    verbose: bool,

    /// Show LLM prompts and responses (verbose LLM debugging)
    #[arg(long)]
    verbose_llm: bool,

    /// Discover tools from MCP servers (requires GITHUB_TOKEN)
    #[arg(long)]
    discover_mcp: bool,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Skip execution (just show the plan)
    #[arg(long)]
    no_execute: bool,

    /// Use fast pattern-based decomposition instead of LLM (faster but less accurate)
    #[arg(long)]
    pattern_mode: bool,

    /// Use embedding-based scoring (default: true, use --no-embeddings to disable)
    #[arg(long, default_value_t = true)]
    use_embeddings: bool,

    /// Disable tool cache (force fresh MCP discovery)
    #[arg(long)]
    no_cache: bool,

    /// Show the full prompt sent to LLM during decomposition
    #[arg(long)]
    show_prompt: bool,

    /// Confirm before each LLM call (shows prompt and waits for Enter)
    #[arg(long)]
    confirm_llm: bool,
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           🧩 Modular Planner Demo                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("📋 Goal: \"{}\"\n", args.goal);

    // Use the builder to set up the environment
    let env = ModularPlannerBuilder::new()
        .with_config(&args.config)
        .with_options(
            args.use_embeddings,
            args.discover_mcp,
            args.no_cache,
            args.pattern_mode,
        )
        .with_safe_exec(true)
        .with_debug_options(args.verbose_llm, args.show_prompt, args.confirm_llm)
        .build()
        .await?;

    let ccos = env.ccos;
    let mut planner = env.planner;
    let intent_graph = env.intent_graph;

    // 6. Plan!
    println!("\n🚀 Planning...\n");

    let plan_result = match planner.plan(&args.goal).await {
        Ok(result) => {
            print_plan_result(&result, args.verbose);

            // Show IntentGraph state
            println!("\n📊 IntentGraph State:");
            let graph = intent_graph.lock().unwrap();
            println!(
                "   Root intent: {}",
                &result.root_intent_id[..40.min(result.root_intent_id.len())]
            );
            println!("   Total intents created: {}", result.intent_ids.len() + 1); // +1 for root

            if let Some(root) = graph.get_intent(&result.root_intent_id) {
                println!("   Root goal: \"{}\"", root.goal);
            }

            Some(result)
        }
        Err(e) => {
            println!("\n❌ Planning failed: {}", e);
            println!("\n💡 Tip: The pattern decomposition only handles specific goal patterns:");
            println!("   - \"X but ask me for Y\"");
            println!("   - \"ask me for X then Y\"");
            println!("   - \"X then Y\"");
            println!("   - \"X and filter/sort by Y\"");
            None
        }
    };

    // 7. Execute!
    if let Some(result) = plan_result {
        if !args.no_execute {
            // Check if plan has pending capabilities that need synthesis
            if result.plan_status == ccos::types::PlanStatus::PendingSynthesis {
                println!("\n🧪 Plan has pending capabilities - starting synthesis...");
                println!("───────────────────────────────────────────────────────────────");

                // Find all NeedsReferral resolutions
                let pending_caps: Vec<_> = result
                    .resolutions
                    .iter()
                    .filter_map(|(intent_id, resolution)| {
                        if let ResolvedCapability::NeedsReferral {
                            reason,
                            suggested_action: _suggested_action,
                        } = resolution
                        {
                            // Find the sub_intent description for this intent_id
                            let description = result
                                .sub_intents
                                .iter()
                                .enumerate()
                                .find(|(idx, _)| {
                                    result
                                        .intent_ids
                                        .get(*idx)
                                        .map(|id| id == intent_id)
                                        .unwrap_or(false)
                                })
                                .map(|(_, si)| si.description.clone())
                                .unwrap_or_else(|| reason.clone());
                            Some((intent_id.clone(), description, reason.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();

                if pending_caps.is_empty() {
                    println!("   ⚠️ No NeedsReferral capabilities found despite PendingSynthesis status.");
                } else {
                    // Create synthesizer with LLM provider using agent_config
                    use ccos::arbiter::llm_provider::{
                        LlmProviderConfig, LlmProviderFactory, LlmProviderType,
                    };
                    use ccos::examples_common::builder::find_llm_profile;

                    let profile_name = "openrouter_free:balanced";
                    let llm_provider = match find_llm_profile(&env.agent_config, profile_name) {
                        Some(profile) => {
                            let api_key = profile.api_key.clone().or_else(|| {
                                profile
                                    .api_key_env
                                    .as_ref()
                                    .and_then(|env| std::env::var(env).ok())
                            });

                            if let Some(key) = api_key {
                                let provider_type = match profile.provider.as_str() {
                                    "openai" => LlmProviderType::OpenAI,
                                    "anthropic" => LlmProviderType::Anthropic,
                                    "openrouter" => LlmProviderType::OpenAI,
                                    _ => LlmProviderType::OpenAI,
                                };

                                let config = LlmProviderConfig {
                                    provider_type,
                                    model: profile.model,
                                    api_key: Some(key),
                                    base_url: profile.base_url,
                                    max_tokens: profile.max_tokens.or(Some(4096)),
                                    temperature: profile.temperature.or(Some(0.0)),
                                    timeout_seconds: Some(60),
                                    retry_config: Default::default(),
                                };

                                match LlmProviderFactory::create_provider(config).await {
                                    Ok(p) => Arc::from(p),
                                    Err(e) => {
                                        println!("   ❌ Failed to create LLM provider: {}", e);
                                        return Ok(());
                                    }
                                }
                            } else {
                                println!("   ❌ No API key found for profile '{}'", profile_name);
                                return Ok(());
                            }
                        }
                        None => {
                            println!(
                                "   ❌ LLM profile '{}' not found in agent_config.toml",
                                profile_name
                            );
                            return Ok(());
                        }
                    };
                    let synthesizer = CapabilitySynthesizer::with_llm_provider(llm_provider);

                    // Create approval queue with file-based storage for persistence
                    let approval_base_path = std::path::PathBuf::from("./storage/approvals");
                    let storage = match FileApprovalStorage::new(approval_base_path.clone()) {
                        Ok(s) => Arc::new(s),
                        Err(e) => {
                            println!("   ⚠️ Failed to create file storage: {}", e);
                            println!("   💡 Make sure ./storage/approvals directory exists");
                            return Ok(());
                        }
                    };
                    let approval_queue = UnifiedApprovalQueue::new(storage);

                    for (_intent_id, description, reason) in &pending_caps {
                        println!("\n🔧 Synthesizing: {}", description);
                        println!("   Reason: {}", reason);

                        // Create synthesis request
                        let cap_name = description
                            .replace(' ', "_")
                            .replace(|c: char| !c.is_alphanumeric() && c != '_', "")
                            .to_lowercase();

                        let request = SynthesisRequest {
                            capability_name: cap_name.clone(),
                            description: Some(description.clone()),
                            input_schema: None,
                            output_schema: None,
                            requires_auth: false,
                            auth_provider: None,
                            context: None,
                        };

                        match synthesizer.synthesize_capability(&request).await {
                            Ok(synth_result) => {
                                println!(
                                    "   ✅ Synthesized: {} ({} chars)",
                                    synth_result.capability.id,
                                    synth_result.implementation_code.len()
                                );

                                // Queue for approval
                                let request_id = approval_queue
                                    .add_synthesis_approval(
                                        synth_result.capability.id.clone(),
                                        synth_result.implementation_code.clone(),
                                        synth_result.safety_passed,
                                        RiskAssessment {
                                            level: if synth_result.safety_passed {
                                                RiskLevel::Medium
                                            } else {
                                                RiskLevel::High
                                            },
                                            reasons: synth_result.warnings.clone(),
                                        },
                                        24,
                                    )
                                    .await
                                    .unwrap_or_else(|_| "unknown".to_string());

                                // Show code and prompt for approval
                                println!("\n📜 Generated RTFS Code:");
                                println!("───────────────────────────────────────────────────────────────");
                                // Show first 500 chars of code
                                let code_preview = if synth_result.implementation_code.len() > 500 {
                                    format!(
                                        "{}\n... ({} more chars)",
                                        &synth_result.implementation_code[..500],
                                        synth_result.implementation_code.len() - 500
                                    )
                                } else {
                                    synth_result.implementation_code.clone()
                                };
                                println!("{}", code_preview);
                                println!("───────────────────────────────────────────────────────────────");
                                println!(
                                    "   Safety Check: {}",
                                    if synth_result.safety_passed {
                                        "✅ PASSED"
                                    } else {
                                        "⚠️ WARNINGS"
                                    }
                                );
                                if !synth_result.warnings.is_empty() {
                                    println!("   Warnings: {:?}", synth_result.warnings);
                                }

                                // Test execution: parse validation with generic static analysis
                                println!("\n🧪 Testing synthesized code...");
                                let test_passed = {
                                    use rtfs::parser::parse_expression;

                                    // Step 1: Verify the code parses correctly
                                    match parse_expression(&synth_result.implementation_code) {
                                        Ok(_expr) => {
                                            println!("   ✅ Parse validation: PASSED");

                                            // Step 2: Generic static analysis for common issues
                                            let code = &synth_result.implementation_code;
                                            let mut issues = Vec::new();

                                            // Check for common RTFS anti-patterns
                                            // 1. Undefined symbol references (symbols ending with ? that aren't defined)
                                            let undefined_refs: Vec<&str> = code
                                                .split_whitespace()
                                                .filter(|word| {
                                                    word.ends_with('?')
                                                        && !word.starts_with('(')
                                                        && !code
                                                            .contains(&format!("(defn {}", word))
                                                        && !code
                                                            .contains(&format!("(fn {} [", word))
                                                })
                                                .collect();

                                            if !undefined_refs.is_empty()
                                                && undefined_refs.len() <= 5
                                            {
                                                // Only warn if there are a few - might be helper fns
                                                issues.push(format!(
                                                    "Potentially undefined functions: {:?}",
                                                    &undefined_refs[..undefined_refs.len().min(3)]
                                                ));
                                            }

                                            // 2. Check for direct side-effects without (call ...)
                                            if code.contains("http://") || code.contains("https://")
                                            {
                                                if !code.contains("(call ") {
                                                    issues.push(
                                                        "Contains URLs without (call ...) wrapper"
                                                            .to_string(),
                                                    );
                                                }
                                            }

                                            // 3. Check for hardcoded secrets
                                            if code.contains("Bearer ")
                                                || code.contains("sk_")
                                                || code.contains("api_key")
                                            {
                                                issues.push(
                                                    "May contain hardcoded credentials".to_string(),
                                                );
                                            }

                                            if issues.is_empty() {
                                                println!(
                                                    "   ✅ Static analysis: No issues detected"
                                                );
                                                println!("   � Capability: {}", description);
                                                println!(
                                                    "   ✨ Code structure looks valid for: {}",
                                                    cap_name
                                                );
                                                true
                                            } else {
                                                println!("   ⚠️ Potential issues found:");
                                                for issue in &issues {
                                                    println!("      - {}", issue);
                                                }
                                                println!("   (These may be false positives if code is valid)");
                                                true // Don't block on warnings
                                            }
                                        }
                                        Err(e) => {
                                            println!("   ❌ Parse validation FAILED: {:?}", e);
                                            false
                                        }
                                    }
                                };

                                println!(
                                    "\n   Test result: {}",
                                    if test_passed {
                                        "✅ READY FOR APPROVAL"
                                    } else {
                                        "⚠️ MAY HAVE ISSUES"
                                    }
                                );

                                print!("\n   Approve this synthesized capability? [y/n]: ");
                                io::stdout().flush().unwrap();

                                let mut input = String::new();
                                io::stdin().read_line(&mut input).unwrap();
                                let answer = input.trim().to_lowercase();

                                if answer == "y" || answer == "yes" {
                                    approval_queue
                                        .approve(
                                            &request_id,
                                            ApprovalAuthority::User("demo-user".to_string()),
                                            Some("Approved by user".to_string()),
                                        )
                                        .await
                                        .ok();
                                    println!(
                                        "   ✅ Approved and saved to {}",
                                        approval_base_path.display()
                                    );
                                } else {
                                    approval_queue
                                        .reject(
                                            &request_id,
                                            ApprovalAuthority::User("demo-user".to_string()),
                                            "Rejected by user".to_string(),
                                        )
                                        .await
                                        .ok();
                                    println!("   ❌ Rejected");
                                }
                            }
                            Err(e) => {
                                println!("   ❌ Synthesis failed: {}", e);
                            }
                        }
                    }

                    println!(
                        "\n💡 Synthesis complete. Approvals saved to {}/",
                        approval_base_path.display()
                    );
                }
            } else {
                // =============================================================
                // Approval Queue Check for High-Risk Capabilities
                // =============================================================
                let storage = Arc::new(InMemoryApprovalStorage::new());
                let approval_queue = UnifiedApprovalQueue::new(storage);

                // Collect high-risk capabilities from resolved steps using manifest metadata
                // Use a set to deduplicate by capability ID (same cap used in multiple steps)
                let mut seen_caps = std::collections::HashSet::new();
                let mut high_risk_caps = Vec::new();
                for (_intent_id, resolution) in &result.resolutions {
                    // Get capability_id from the resolution enum
                    if let Some(cap_id) = resolution.capability_id() {
                        // Skip if we've already processed this capability
                        if seen_caps.contains(cap_id) {
                            continue;
                        }
                        seen_caps.insert(cap_id.to_string());

                        // Query the capability manifest from the marketplace
                        if let Some(manifest) =
                            ccos.capability_marketplace.get_capability(cap_id).await
                        {
                            // Safe effects that don't require approval
                            const SAFE_EFFECTS: &[&str] =
                                &["network", "compute", "read", "output", "pure", "llm"];

                            // Check if any effect is NOT in the safe list
                            let has_unsafe_effects = manifest.effects.iter().any(|eff| {
                                let norm = eff.trim().to_lowercase();
                                let norm = norm.strip_prefix(':').unwrap_or(&norm);
                                !SAFE_EFFECTS.contains(&norm) && !norm.is_empty()
                            });

                            // High risk if:
                            // 1. Explicitly marked as high risk in metadata, OR
                            // 2. Has unsafe effects (fs, delete, write, system, etc.)
                            let is_high_risk = manifest
                                .metadata
                                .get("risk_level")
                                .map(|level| level == "high")
                                .unwrap_or(false)
                                || has_unsafe_effects;

                            if is_high_risk {
                                // Use capability description from manifest, or fallback to cap_id
                                let description = manifest.description.clone();

                                high_risk_caps.push((cap_id.to_string(), description, manifest));
                            }
                        }
                    }
                }

                let mut all_approved = true;

                if !high_risk_caps.is_empty() {
                    println!("\n⚠️  High-Risk Capabilities Detected!");
                    println!("───────────────────────────────────────────────────────────────");

                    for (cap_id, description, manifest) in &high_risk_caps {
                        // Queue for approval
                        let effects: Vec<String> = manifest.effects.clone();
                        let risk_level = manifest
                            .metadata
                            .get("risk_level")
                            .cloned()
                            .unwrap_or_else(|| "high".to_string());

                        let request_id = approval_queue
                            .add_effect_approval(
                                cap_id.to_string(),
                                effects.clone(),
                                format!("Execute: {}", description),
                                RiskAssessment {
                                    level: if risk_level == "high" {
                                        RiskLevel::High
                                    } else {
                                        RiskLevel::Medium
                                    },
                                    reasons: vec![format!("Effect-based risk: {:?}", effects)],
                                },
                                24,
                            )
                            .await
                            .unwrap_or_else(|_| "unknown".to_string());

                        println!("\n🔒 Approval Required:");
                        println!("   Capability: {}", cap_id);
                        println!("   Intent: {}", description);
                        println!("   Effects: {:?}", effects);
                        println!("   Risk Level: {}", risk_level.to_uppercase());
                        print!("\n   Approve execution? [y/n]: ");
                        io::stdout().flush().unwrap();

                        let mut input = String::new();
                        io::stdin().read_line(&mut input).unwrap();
                        let answer = input.trim().to_lowercase();

                        if answer == "y" || answer == "yes" {
                            approval_queue
                                .approve(
                                    &request_id,
                                    ApprovalAuthority::User("demo-user".to_string()),
                                    Some("Approved by user".to_string()),
                                )
                                .await
                                .ok();
                            println!("   ✅ Approved");
                        } else {
                            approval_queue
                                .reject(
                                    &request_id,
                                    ApprovalAuthority::User("demo-user".to_string()),
                                    "Rejected by user".to_string(),
                                )
                                .await
                                .ok();
                            println!("   ❌ Rejected");
                            all_approved = false;
                        }
                    }
                }

                if !all_approved {
                    println!("\n❌ Execution aborted - not all capabilities were approved.");
                } else {
                    println!("\n⚡ Executing Plan...");
                    println!("───────────────────────────────────────────────────────────────");

                    let plan_obj = ccos::types::Plan {
                        plan_id: format!("modular-plan-{}", uuid::Uuid::new_v4()),
                        name: Some("Modular Plan".to_string()),
                        body: ccos::types::PlanBody::Rtfs(result.rtfs_plan.clone()),
                        intent_ids: result.intent_ids.clone(),
                        status: result.plan_status,
                        ..Default::default()
                    };

                    let context = RuntimeContext::full();
                    match ccos.validate_and_execute_plan(plan_obj, &context).await {
                        Ok(exec_result) => {
                            println!("\n🏁 Execution Result:");
                            println!("   Success: {}", exec_result.success);

                            // Format output nicely
                            let output_str = value_to_string(&exec_result.value);
                            println!("   Result: {}", output_str);

                            if !exec_result.success {
                                if let Some(err) = exec_result.metadata.get("error") {
                                    println!("   Error: {:?}", err);
                                }
                            }
                        }
                        Err(e) => {
                            println!("\n❌ Execution Failed: {}", e);
                        }
                    }
                }
            } // end else (plan is ready to execute)
        }
    }

    println!("\n✅ Demo complete!");
    Ok(())
}

/// Convert RTFS value to string for display
fn value_to_string(v: &rtfs::runtime::values::Value) -> String {
    format!("{:?}", v)
}

/// Print the plan result
fn print_plan_result(result: &PlanResult, verbose: bool) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("📋 Plan Result");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Show resolved steps
    println!("📝 Resolved Steps ({}):", result.intent_ids.len());
    for (i, intent_id) in result.intent_ids.iter().enumerate() {
        if let Some(resolution) = result.resolutions.get(intent_id) {
            let (status, cap_id) = match resolution {
                ResolvedCapability::Local { capability_id, .. } => {
                    ("Local", capability_id.as_str())
                }
                ResolvedCapability::Remote { capability_id, .. } => {
                    ("Remote", capability_id.as_str())
                }
                ResolvedCapability::BuiltIn { capability_id, .. } => {
                    ("BuiltIn", capability_id.as_str())
                }
                ResolvedCapability::Synthesized { capability_id, .. } => {
                    ("Synth", capability_id.as_str())
                }
                ResolvedCapability::NeedsReferral { reason, .. } => ("Referral", reason.as_str()),
            };
            println!("   {}. [{}] {}", i + 1, status, cap_id);
        }
    }

    // Show generated RTFS plan
    println!("\n📜 Generated RTFS Plan:");
    println!("───────────────────────────────────────────────────────────────");
    println!("{}", result.rtfs_plan);
    println!("───────────────────────────────────────────────────────────────");

    // Show trace if verbose
    if verbose {
        println!("\n🔍 Planning Trace:");
        for event in &result.trace.events {
            match event {
                TraceEvent::DecompositionStarted { strategy } => {
                    println!("   → Decomposition started with strategy: {}", strategy);
                }
                TraceEvent::DecompositionCompleted {
                    num_intents,
                    confidence,
                } => {
                    println!(
                        "   ✓ Decomposition completed: {} intents, confidence: {:.2}",
                        num_intents, confidence
                    );
                }
                TraceEvent::IntentCreated {
                    intent_id,
                    description,
                } => {
                    println!(
                        "   + Intent created: {} - \"{}\"",
                        &intent_id[..20.min(intent_id.len())],
                        description
                    );
                }
                TraceEvent::EdgeCreated {
                    from,
                    to,
                    edge_type,
                } => {
                    println!(
                        "   ⟶ Edge: {} -> {} ({})",
                        &from[..16.min(from.len())],
                        &to[..16.min(to.len())],
                        edge_type
                    );
                }
                TraceEvent::ResolutionStarted { intent_id } => {
                    println!("   🔍 Resolving: {}", &intent_id[..20.min(intent_id.len())]);
                }
                TraceEvent::ResolutionCompleted {
                    intent_id,
                    capability,
                } => {
                    println!(
                        "   ✓ Resolved: {} → {}",
                        &intent_id[..16.min(intent_id.len())],
                        capability
                    );
                }
                TraceEvent::ResolutionFailed { intent_id, reason } => {
                    println!(
                        "   ✗ Failed: {} - {}",
                        &intent_id[..16.min(intent_id.len())],
                        reason
                    );
                }
                TraceEvent::LlmCalled {
                    model,
                    tokens_prompt,
                    tokens_response,
                    duration_ms,
                    ..
                } => {
                    println!(
                        "   🤖 LLM called: {} ({}+{} tokens, {}ms)",
                        model, tokens_prompt, tokens_response, duration_ms
                    );
                }
                TraceEvent::DiscoverySearchCompleted { query, num_results } => {
                    println!(
                        "   🔍 Discovery search: '{}' ({} results)",
                        query, num_results
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pattern_decomposition() {
        use ccos::intent_graph::{config::IntentGraphConfig, IntentGraph};
        use std::sync::Mutex;

        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage()).unwrap(),
        ));

        // Mock catalog for test (since we can't easily spin up CCOS here)
        struct MockCatalog;
        #[async_trait::async_trait(?Send)]
        impl CapabilityCatalog for MockCatalog {
            async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
                vec![]
            }
            async fn get_capability(&self, _id: &str) -> Option<CapabilityInfo> {
                None
            }
            async fn search(&self, _query: &str, _limit: usize) -> Vec<CapabilityInfo> {
                vec![]
            }
        }
        let catalog = Arc::new(MockCatalog);

        let mut planner = ModularPlanner::new(
            Box::new(PatternDecomposition::new()),
            Box::new(CatalogResolution::new(catalog)),
            intent_graph,
        );

        let result = planner
            .plan("list issues but ask me for page size")
            .await
            .unwrap();

        assert_eq!(result.intent_ids.len(), 2);
        assert!(result.rtfs_plan.contains("ccos.user.ask"));
    }
}
