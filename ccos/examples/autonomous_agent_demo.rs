//! Autonomous Agent Demo (Iterative & Recursive)
//!
//! This example demonstrates an advanced, self-evolving autonomous agent that:
//! 1. Takes a high-level goal from the user.
//! 2. Iteratively decomposes it into steps using the Arbiter.
//! 3. Resolves capabilities for each step (Local -> Semantic Search -> MCP Registry).
//! 4. Recursively plans for missing capabilities that can't be found directly.
//! 5. Synthesizes missing capabilities using LLM-generated RTFS code (Phase B).
//! 6. Constructs a final executable RTFS plan.
//! 7. Traces the decision process.
//!
//! **Phase B (Implemented):** True Code Synthesis
//! - When a capability is missing, the agent asks the LLM to write RTFS code
//! - Generated code is validated (parse check) and registered dynamically
//! - The synthesized capability can be executed just like any other capability
//!
//! **Phase C (Planned):** Data Flow Adapters
//! - When capabilities have known input/output schemas, create adapters
//! - Transform previous step output to match next step input requirements
//! - Example: remote.list_items returns {:records [...]}, but filter expects {:items [...]}
//! - Adapter: (fn [prev-output] {:items (:records prev-output)})
//!
//! Usage:
//!   cargo run --example autonomous_agent_demo -- --goal "find the issues of repository ccos and user mandubian and filter them to keep only those containing RTFS"

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;

use clap::Parser;
use ccos::CCOS;
use rtfs::config::types::AgentConfig;
use ccos::arbiter::DelegatingArbiter;
use ccos::catalog::{CatalogService, CatalogFilter, CatalogEntryKind};
use ccos::capability_marketplace::{CapabilityMarketplace, CapabilityManifest};
use ccos::synthesis::mcp_session::{MCPSessionManager, MCPServerInfo};
use ccos::synthesis::mcp_registry_client::McpRegistryClient;
use ccos::synthesis::mcp_introspector::{MCPIntrospector, DiscoveredMCPTool};
use ccos::discovery::capability_matcher::{
    calculate_action_verb_match_score, calculate_description_match_score, extract_action_verbs,
};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::runtime::error::RuntimeResult;

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
struct Args {
    /// Natural language goal
    #[arg(long, default_value = "Find the weather in Paris and filter for rain")]
    goal: String,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Optional LLM profile name
    #[arg(long)]
    profile: Option<String>,

    /// Simulate a runtime error to test the repair loop
    #[arg(long)]
    simulate_error: bool,
    
    /// Disable mock fallback to force synthesis for missing capabilities
    #[arg(long)]
    no_mock: bool,
}

// ============================================================================
// Data Structures for Planning
// ============================================================================

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct PlannedStep {
    description: String,
    capability_hint: String,
}

#[derive(Debug, serde::Deserialize)]
struct Decomposition {
    steps: Vec<PlannedStep>,
}

#[derive(Debug, Clone)]
enum ResolutionStatus {
    ResolvedLocal(String, HashMap<String, String>), // ID, Args
    ResolvedRemote(String, HashMap<String, String>), // ID, Args (installed from MCP)
    ResolvedSynthesized(String, HashMap<String, String>), // ID, Args (generated)
    NeedsSubPlan(String, String), // Goal, Hint (Recursive)
    Failed(String), // Reason
}

#[derive(Debug, serde::Serialize)]
struct PlanningTrace {
    goal: String,
    decisions: Vec<TraceEvent>,
}

#[derive(Debug, serde::Serialize)]
enum TraceEvent {
    Decomposition(Vec<PlannedStep>),
    ResolutionAttempt { step: String, status: String },
    MCPDiscovery { hint: String, found: bool },
    Synthesis { capability: String, success: bool },
    RecursiveSubPlan { parent_step: String, sub_goal: String },
}

// ============================================================================
// Main Planner Loop
// ============================================================================

struct IterativePlanner {
    _ccos: Arc<CCOS>,
    arbiter: Arc<DelegatingArbiter>,
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
    trace: PlanningTrace,
    simulate_error: bool,
    no_mock: bool,
}

impl IterativePlanner {
    fn new(ccos: Arc<CCOS>, simulate_error: bool, no_mock: bool) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let arbiter = ccos.get_delegating_arbiter()
            .ok_or::<Box<dyn Error + Send + Sync>>("Delegating arbiter not available".into())?;
        let marketplace = ccos.get_capability_marketplace();
        let catalog = ccos.get_catalog();

        Ok(Self {
            _ccos: ccos,
            arbiter: arbiter.clone(),
            marketplace,
            catalog,
            trace: PlanningTrace {
                goal: "Unknown".to_string(),
                decisions: Vec::new(),
            },
            simulate_error,
            no_mock,
        })
    }

    // Recursive async function requires manual boxing
    fn solve<'a>(&'a mut self, goal: &'a str, depth: usize) -> Pin<Box<dyn Future<Output = Result<String, Box<dyn Error + Send + Sync>>> + 'a>> {
        Box::pin(async move {
            if depth > 5 {
                return Err("Max recursion depth exceeded".into());
            }
            self.trace.goal = goal.to_string();
            println!("\n🧠 Solving Goal (Depth {}): \"{}\"", depth, goal);

            // 1. Decompose
            let steps = self.decompose(goal).await?;
            self.trace.decisions.push(TraceEvent::Decomposition(steps.clone()));

            // Build nested let bindings with context accumulation for data flow
            let mut step_bindings = Vec::new();
            let mut context_entries: Vec<String> = Vec::new(); // Track all previous steps for context map

            for (i, step) in steps.iter().enumerate() {
                println!("\n  👉 Step {}: {} (Hint: {})", i+1, step.description, step.capability_hint);
                
                // 2. Resolve
                let status = self.resolve_step(step).await?;
                
                self.trace.decisions.push(TraceEvent::ResolutionAttempt { 
                    step: step.description.clone(), 
                    status: format!("{:?}", status) 
                });

                let (capability_id, args) = match status {
                    ResolutionStatus::ResolvedLocal(id, a) => {
                        println!("     ✅ Resolved Local: {}", id);
                        (id, a)
                    },
                    ResolutionStatus::ResolvedRemote(id, a) => {
                        println!("     ✅ Resolved Remote (Installed): {}", id);
                        (id, a)
                    },
                    ResolutionStatus::ResolvedSynthesized(id, a) => {
                        println!("     ✅ Resolved Synthesized: {}", id);
                        (id, a)
                    },
                    ResolutionStatus::NeedsSubPlan(sub_goal, _hint) => {
                        println!("     🔄 Complex Step -> Triggering Recursive Sub-Planning...");
                        self.trace.decisions.push(TraceEvent::RecursiveSubPlan {
                            parent_step: step.description.clone(),
                            sub_goal: sub_goal.clone()
                        });
                        
                        // Recursive call!
                        let sub_plan_rtfs = self.solve(&sub_goal, depth + 1).await?;
                        step_bindings.push(("subplan".to_string(), sub_plan_rtfs));
                        continue;
                    },
                    ResolutionStatus::Failed(reason) => {
                        println!("     ❌ Failed: {}", reason);
                        return Err(format!("Planning failed at step '{}': {}", step.description, reason).into());
                    }
                };

                // Generate call with data flow (Phase E: Context-Aware Adapter Synthesis)
                let step_var = format!("step_{}", i + 1);
                
                let call_expr = if !context_entries.is_empty() {
                    // If we have context from previous steps, try to adapt it to the current step
                    if args.is_empty() {
                        match self.synthesize_adapter_with_context(&context_entries, &capability_id, &step.description).await {
                            Ok(adapter_expr) => adapter_expr,
                            Err(e) => {
                                println!("     ⚠️  Context-aware adapter synthesis failed: {}. Falling back to direct call.", e);
                                self.generate_call(&capability_id, args)
                            }
                        }
                    } else {
                        // If args are present, assume they are sufficient
                        self.generate_call(&capability_id, args)
                    }
                } else {
                    // First step or no previous output
                    self.generate_call(&capability_id, args)
                };

                step_bindings.push((step_var.clone(), call_expr));
                context_entries.push(step_var.clone());
            }

            // Build nested let expression from bindings
            if step_bindings.is_empty() {
                return Ok("nil".to_string());
            }
            
            let plan_expr = if step_bindings.len() == 1 {
                // Single step - no binding needed, just the call
                step_bindings[0].1.clone()
            } else {
                // Multiple steps - build nested lets from innermost to outermost
                // Start with the last step's variable as the final result
                let last_var = &step_bindings[step_bindings.len() - 1].0;
                let mut expr = last_var.clone();
                
                // Wrap from last step to first step (building from inside out)
                for (var, call_expr) in step_bindings.iter().rev() {
                    if var == last_var {
                        // Last step - bind the call and use the variable as result
                        expr = format!("(let [{} {}]\n  {})", var, call_expr, expr);
                    } else {
                        // Earlier step - bind and continue nesting
                        expr = format!("(let [{} {}]\n  {})", var, call_expr, expr);
                    }
                }
                expr
            };

            Ok(plan_expr)
        })
    }

    async fn decompose(&self, goal: &str) -> Result<Vec<PlannedStep>, Box<dyn Error + Send + Sync>> {
        let prompt = format!(
            r#"You are an expert planner. Decompose the following goal into a sequence of logical steps.
For each step, provide a description and a short "capability hint" that looks like a tool ID (e.g. "service.action", "data.transform").

Goal: "{}"

Respond ONLY with a JSON object in this format:
{{
  "steps": [
    {{ "description": "Step description", "capability_hint": "service.action" }}
  ]
}}
"#,
            goal
        );

        let response = self.arbiter.generate_raw_text(&prompt).await?;
        let json_str = extract_json(&response);
        let decomposition: Decomposition = serde_json::from_str(json_str)?;
        Ok(decomposition.steps)
    }

    async fn resolve_step(&mut self, step: &PlannedStep) -> Result<ResolutionStatus, Box<dyn Error + Send + Sync>> {
        // Resolution strategy:
        // 1. Check if hint matches any MCP server in overrides.json
        // 2. If yes, try MCP first
        // 3. Otherwise, try local first, then MCP as fallback
        
        let hint_has_mcp_override = resolve_server_url_from_overrides(&step.capability_hint).is_some();
        
        if hint_has_mcp_override {
            // Try MCP first for hints that match known MCP servers
            println!("     🔍 Hint '{}' matches MCP override. Trying MCP first...", step.capability_hint);
            if let Some(mcp_result) = self.try_mcp_resolution(step).await? {
                return Ok(mcp_result);
            }
            println!("     ⚠️  MCP resolution failed. Falling back to local search...");
        }
        
        // A. Semantic Search (Local)
        let query = format!("{} {}", step.capability_hint, step.description);
        let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
        let hits = self.catalog.search_semantic(&query, Some(&filter), 5);

        let mut local_candidates = Vec::new();
        for hit in hits {
            if let Some(cap) = self.marketplace.get_capability(&hit.entry.id).await {
                local_candidates.push(cap);
            }
        }

        // Try to select from local candidates
        if !local_candidates.is_empty() {
            if let Some((id, args)) = self.try_select_from_candidates(&local_candidates, &step.description).await? {
                return Ok(ResolutionStatus::ResolvedLocal(id, args));
            }
            println!("     ⚠️  Local candidates rejected by LLM. Trying registry...");
        }

        // B. If no local (or rejected), try MCP Registry (Remote)
        if !hint_has_mcp_override {
            // Only try MCP here if we didn't already try it above
            println!("     🔍 Searching MCP Registry for '{}' (Context: '{}')...", step.capability_hint, step.description);
            if let Some(mcp_result) = self.try_mcp_resolution(step).await? {
                return Ok(mcp_result);
            }
        }

        // C. If registry fails, try Synthesis
        println!("     🧪 Registry failed. Attempting to synthesize capability for '{}'...", step.description);
        if let Some(synth_cap) = self.try_synthesize(&step.description, &step.capability_hint).await? {
             println!("     ✨ Synthesized new capability: {}", synth_cap.id);
             // For synthesized capabilities, we need to extract arguments more intelligently
             // The synthesized function expects an 'args' map, so we need to construct it from the step description
             let synth_cap_id = synth_cap.id.clone();
             let candidates = vec![synth_cap];
             
             // Enhanced prompt for synthesized capabilities: they typically need data from previous steps
             let enhanced_goal = format!(
                 "{}. Note: This is a synthesized capability that operates on data. Extract any parameters from the description, but note that the actual data to process will come from previous step outputs.",
                 step.description
             );
             if let Some((id, args)) = self.try_select_from_candidates(&candidates, &enhanced_goal).await? {
                return Ok(ResolutionStatus::ResolvedSynthesized(id, args));
            } else {
                // If LLM fails to extract args, use empty map (will fail at runtime, but that's Phase C issue)
                println!("     ⚠️  Could not extract arguments for synthesized capability. Using empty args (may fail at runtime).");
                return Ok(ResolutionStatus::ResolvedSynthesized(synth_cap_id, HashMap::new()));
            }
        }

        // D. If we are here, either no candidates found OR LLM rejected them.
        Ok(ResolutionStatus::NeedsSubPlan(step.description.clone(), step.capability_hint.clone()))
    }

    async fn try_synthesize(&mut self, description: &str, hint: &str) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        // Phase B: Real LLM Synthesis
        println!("     🧪 Synthesizing RTFS implementation for: {}", description);
        
        let prompt = format!(
            r#"You are an expert RTFS (Lisp-like) programmer.
Write an anonymous RTFS function (using `fn`) that performs the following task:
"{}"

The function will receive a SINGLE argument `args` which is a Map containing input parameters.
The function should return the result of the operation.

Examples:
- Task: "Filter items to keep only active ones"
  Code: (fn [args] (filter (fn [item] (= (:status item) "active")) (:items args)))

- Task: "Extract the 'id' field from a list of objects"
  Code: (fn [args] (map (fn [item] (:id item)) (:data args)))

- Task: "Search for text containing a substring"
  Code: (fn [args] (filter (fn [item] (contains? (:text item) (:query args))) (:items args)))

Respond ONLY with the valid RTFS code for the function. Do not add markdown formatting, code fences, or explanation.
"#,
            description
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;
        
        // Clean up: remove markdown code fences, trim whitespace
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            // Remove opening fence (may have language tag like ```rtfs or ```lisp)
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end+1..];
    } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len()-3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();
        
        // Remove language tags if present
        if clean_code.starts_with("rtfs") || clean_code.starts_with("lisp") {
            if let Some(space) = clean_code.find(' ') {
                clean_code = &clean_code[space+1..];
            }
        }
        clean_code = clean_code.trim();
        
        println!("     📝 Generated Code: {}", clean_code);

        // Validate: Try parsing the code to catch syntax errors early
        // We only check if the code parses, not if it executes successfully
        // Wrap in parentheses to make it a valid expression
        let test_parse = format!("({})", clean_code);
        let mut final_code = clean_code.to_string();
        let mut parse_valid = rtfs::parser::parse(&test_parse).is_ok();
        let mut repair_attempts = 0;
        const MAX_REPAIR_ATTEMPTS: usize = 2;
        
        // Auto-repair loop: if parse fails, ask LLM to fix it
        while !parse_valid && !final_code.is_empty() && repair_attempts < MAX_REPAIR_ATTEMPTS {
            repair_attempts += 1;
            println!("     🔧 Parse validation failed. Attempting auto-repair (attempt {}/{})...", repair_attempts, MAX_REPAIR_ATTEMPTS);
            
            let repair_prompt = format!(
                r#"The following RTFS code failed to parse. Please fix the syntax errors and return ONLY the corrected code.

Original code:
{}
Error: Parse validation failed

Task description: "{}"

Respond ONLY with the corrected RTFS function code. Do not add markdown formatting or explanation.
"#,
                final_code, description
            );
            
            let repaired_code = self.arbiter.generate_raw_text(&repair_prompt).await?;
            let mut repaired_clean = repaired_code.trim();
            
            // Clean up repaired code
            if repaired_clean.starts_with("```") {
                if let Some(end) = repaired_clean.find('\n') {
                    repaired_clean = &repaired_clean[end+1..];
                } else {
                    repaired_clean = &repaired_clean[3..];
                }
            }
            if repaired_clean.ends_with("```") {
                repaired_clean = &repaired_clean[..repaired_clean.len()-3];
            }
            repaired_clean = repaired_clean.trim().trim_matches('`').trim();
            
            if repaired_clean.starts_with("rtfs") || repaired_clean.starts_with("lisp") {
                if let Some(space) = repaired_clean.find(' ') {
                    repaired_clean = &repaired_clean[space+1..];
                }
            }
            repaired_clean = repaired_clean.trim();
            
            if !repaired_clean.is_empty() && repaired_clean != final_code {
                final_code = repaired_clean.to_string();
                println!("     📝 Repaired Code: {}", final_code);
                
                // Test the repaired code: just check if it parses
                let test_parse_repaired = format!("({})", final_code);
                parse_valid = rtfs::parser::parse(&test_parse_repaired).is_ok();
                
                if parse_valid {
                    println!("     ✅ Auto-repair succeeded!");
                }
            } else {
                println!("     ⚠️  LLM returned same code or empty. Stopping repair attempts.");
                break;
            }
        }
        
        if !parse_valid && !final_code.is_empty() {
            println!("     ⚠️  Code still fails parse validation after {} repair attempts, but proceeding anyway...", repair_attempts);
        }
        
        // Fallback if LLM refuses or returns empty (for demo stability)
        if final_code.is_empty() || final_code.contains("I cannot") || final_code.contains("I'm sorry") {
            println!("     ⚠️  LLM failed to synthesize code. Using fallback mock.");
            final_code = "(fn [args] (str \"Fallback Result for: \" args))".to_string();
        }

        let id = format!("ccos.synthesized.{}", hint.replace(".", "_").replace(" ", "_"));
        
        self.trace.decisions.push(TraceEvent::Synthesis { 
            capability: id.clone(), 
            success: !final_code.contains("Fallback")
        });
        
        let code_clone = final_code.clone();
        
        // Save synthesized capability to file for inspection
        save_synthesized_capability(&id, &final_code, description, hint);
        
        // Register a handler that executes this code dynamically
        if self.marketplace.get_capability(&id).await.is_none() {
             let id_for_handler = id.clone();
             let handler = Arc::new(move |args: &Value| -> RuntimeResult<Value> {
                 println!("     [{}] 🧪 Executing synthesized RTFS code...", id_for_handler);
                 
                 // Instantiate a fresh ephemeral runtime for this execution
                 let module_registry = Arc::new(rtfs::runtime::ModuleRegistry::new());
                 let runtime = rtfs::runtime::Runtime::new_with_tree_walking_strategy(module_registry);
                 
                 // Convert Value to RTFS literal representation
                 // For now, we'll use a simple approach: serialize args as a map literal
                 let args_rtfs = value_to_rtfs_literal(args);
                 
                 // Construct the program: ((fn [args] ...) <input_args>)
                 let program = format!("(({}) {})", code_clone, args_rtfs);
                 
                 match runtime.evaluate(&program) {
                     Ok(v) => {
                         println!("     ✅ Synthesis execution succeeded");
                         Ok(v)
                     },
                     Err(e) => {
                         println!("     ❌ Synthesis Execution Error: {}", e);
                         // Return error but don't crash - let the planner handle it
                         Err(e)
                     }
                 }
             });

             let _ = self.marketplace.register_local_capability(
                 id.clone(),
                 format!("Synthesized: {}", hint),
                 description.to_string(),
                 handler
             ).await;
        }
        
        return Ok(self.marketplace.get_capability(&id).await);
    }

    async fn synthesize_adapter_with_context(&self, context_vars: &[String], target_capability_id: &str, target_description: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        println!("     🔌 Synthesizing context-aware data adapter for '{}'...", target_capability_id);
        
        // Build context map description for the prompt
        let context_desc = if context_vars.len() == 1 {
            format!("The variable '{}' contains the result from the previous step.", context_vars[0])
        } else {
            let vars_list: Vec<String> = context_vars.iter().map(|v| format!("'{}'", v)).collect();
            format!("Variables {} contain results from previous steps (in order).", vars_list.join(", "))
        };
        
        // Determine which context variable(s) to use
        let prev_var = context_vars.last().unwrap(); // Most recent by default
        let all_vars_available = context_vars.join(", ");
        
        if self.simulate_error {
            println!("     ⚠️  SIMULATING ERROR: Generating broken adapter code...");
            return Ok("(call \"non_existent_function_to_force_crash\" {})".to_string());
        }
        
        let prompt = format!(
            r#"You are an expert RTFS programmer. Generate ONLY the RTFS code to call '{}' with data from previous steps.

Task: {}
{}

Available context variables: {}
Most recent result is in: {}

If the task requires extracting specific fields (e.g., "get the ID from the first item"), generate code that:
1. Accesses the appropriate context variable
2. Extracts the required data (use 'get', 'first', 'nth', etc.)
3. Passes it to the capability call

Examples:
- Simple pass-through: (call "{}" {{:data {}}})
- Extract field: (call "{}" {{:query (get {} :field_name)}})
- First item ID: (call "{}" {{:id (get (first (get {} :items)) :id)}})
- Filter operation: (call "{}" {{:data {} :filter "condition"}})

Respond with ONLY the RTFS expression - no markdown, no explanation.
"#,
            target_capability_id,
            target_description,
            context_desc,
            all_vars_available,
            prev_var,
            target_capability_id, prev_var,
            target_capability_id, prev_var,
            target_capability_id, prev_var,
            target_capability_id, prev_var
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;
        
        // Clean up code
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end+1..];
            } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len()-3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();
        
        // Remove language tags
        if clean_code.starts_with("rtfs") || clean_code.starts_with("lisp") || clean_code.starts_with("clojure") {
            if let Some(space) = clean_code.find(' ') {
                clean_code = &clean_code[space+1..];
            }
        }
        clean_code = clean_code.trim();
        
        Ok(clean_code.to_string())
    }

    async fn synthesize_adapter(&self, prev_var: &str, target_capability_id: &str, target_description: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        // Legacy single-variable adapter - delegate to context-aware version
        self.synthesize_adapter_with_context(&[prev_var.to_string()], target_capability_id, target_description).await
    }

    #[allow(dead_code)]
    async fn synthesize_adapter_old(&self, prev_var: &str, target_capability_id: &str, target_description: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        println!("     🔌 Synthesizing data adapter for '{}'...", target_capability_id);
        
        if self.simulate_error {
            println!("     ⚠️  SIMULATING ERROR: Generating broken adapter code...");
            return Ok("(call \"non_existent_function_to_force_crash\" {})".to_string());
        }

        let prompt = format!(
            r#"You are an expert RTFS programmer.
We need to pass data from a previous step (variable `{}`) to a tool named `{}`.
The tool's description is: "{}".

Write an RTFS expression that calls the tool with the correct arguments, using the data from `{}`.
The previous step output is likely a Map or List. You may need to wrap it in a map with a specific key (e.g. :data, :items, :records).

Examples:
- Tool: "filter_records", Input: `step_1` (List of records)
  Expression: (call "filter_records" {{:records step_1}})

- Tool: "summarize_text", Input: `step_1` (String)
  Expression: (call "summarize_text" {{:text step_1}})

- Tool: "process_data", Input: `step_1` (Map)
  Expression: (call "process_data" step_1)

Respond ONLY with the RTFS expression.
"#,
            prev_var, target_capability_id, target_description, prev_var
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;
        
        // Clean up code
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end+1..];
            } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len()-3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();
        
        Ok(clean_code.to_string())
    }

    async fn try_select_from_candidates(&self, candidates: &[CapabilityManifest], goal: &str) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error + Send + Sync>> {
        let tool_descriptions: Vec<String> = candidates.iter()
            .map(|c| format!("{}: {}", c.id, c.description))
            .collect();
        
        self.select_tool_robust(goal, &tool_descriptions).await
    }

    /// Try to resolve a step using MCP registry/server
    async fn try_mcp_resolution(&mut self, step: &PlannedStep) -> Result<Option<ResolutionStatus>, Box<dyn Error + Send + Sync>> {
        println!("     🔍 Searching MCP Registry for '{}' (Context: '{}')...", step.capability_hint, step.description);
        
        if let Some(installed_cap) = self.try_install_from_registry(&step.capability_hint, &step.description).await? {
            println!("     📦 Found MCP capability: {}", installed_cap.id);
            let remote_candidates = vec![installed_cap];
            if let Some((id, args)) = self.try_select_from_candidates(&remote_candidates, &step.description).await? {
                return Ok(Some(ResolutionStatus::ResolvedRemote(id, args)));
            } else {
                println!("     ⚠️  MCP capability rejected by LLM.");
            }
        } else {
            println!("     ❌ MCP Registry search returned no results.");
        }
        
        Ok(None)
    }

    async fn try_install_from_registry(&mut self, hint: &str, description: &str) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        let client = McpRegistryClient::new();
        let search_query = if hint.contains(".") { hint } else { description };
        let servers = client.search_servers(search_query).await.unwrap_or_default();
        
        // Try to find a matching MCP server configuration
        let mcp_server_config = self.find_mcp_server_config(hint, &servers);
        
        if let Some((server_url, auth_headers, server_name)) = mcp_server_config {
            // Attempt real MCP discovery
            println!("     🔌 Attempting real MCP connection to: {}", server_url);
            match self.try_real_mcp_discovery(&server_url, auth_headers, hint, &server_name).await {
                Ok(Some(manifest)) => {
                    println!("     ✅ Real MCP capability discovered: {}", manifest.id);
                    self.trace.decisions.push(TraceEvent::MCPDiscovery { hint: hint.to_string(), found: true });
                    return Ok(Some(manifest));
                },
                Ok(None) => {
                    println!("     ⚠️  Real MCP connection succeeded but tool not found");
                },
                Err(e) => {
                    println!("     ⚠️  Real MCP connection failed: {}. Falling back to mock.", e);
                }
            }
        }
        
        // Fallback to generic mock if real MCP fails or no server config found
        // Skip mock if --no-mock flag is set (force synthesis instead)
        if self.no_mock {
            println!("     🚫 Mock fallback disabled (--no-mock). Will try synthesis.");
            self.trace.decisions.push(TraceEvent::MCPDiscovery { hint: hint.to_string(), found: false });
            return Ok(None);
        }
        
        let should_install = !servers.is_empty() || hint.starts_with("mcp.") || hint.contains(".");
        
        if should_install {
            let cap_id = if hint.contains(".") { hint.to_string() } else { format!("mcp.{}", hint.replace(" ", "_")) };
            
            if self.marketplace.get_capability(&cap_id).await.is_some() {
                return Ok(self.marketplace.get_capability(&cap_id).await);
            }

            self.trace.decisions.push(TraceEvent::MCPDiscovery { hint: hint.to_string(), found: true });
            println!("     🌐 [Demo] Installing generic mock capability: {}", cap_id);
            
            self.install_generic_mock_capability(&cap_id, description).await?;
            
            return Ok(self.marketplace.get_capability(&cap_id).await);
        }

        self.trace.decisions.push(TraceEvent::MCPDiscovery { hint: hint.to_string(), found: false });
        Ok(None)
    }

    /// Find a matching MCP server configuration from agent config or overrides
    /// Returns (url, auth_headers, server_name)
    fn find_mcp_server_config(&self, hint: &str, servers: &[ccos::synthesis::mcp_registry_client::McpServer]) -> Option<(String, Option<std::collections::HashMap<String, String>>, String)> {
        // 1. First, check overrides.json for matching server
        if let Some((server_url, server_name)) = resolve_server_url_from_overrides(hint) {
            println!("     📁 Found MCP server in overrides: {} (namespace: {})", server_url, server_name);
            let auth_headers = get_mcp_auth_headers();
            return Some((server_url, auth_headers, server_name));
        }
        
        // 2. Check if any server from registry has a usable remote URL
        for server in servers {
            if let Some(remotes) = &server.remotes {
                if let Some(url) = ccos::synthesis::mcp_registry_client::McpRegistryClient::select_best_remote_url(remotes) {
                    println!("     🌐 Found MCP server in registry: {} ({})", server.name, url);
                    let auth_headers = get_mcp_auth_headers();
                    // Use server name from registry
                    return Some((url, auth_headers, server.name.clone()));
                }
            }
        }
        
        // 3. Fallback: check environment variable for explicit endpoint
        if hint.contains("github") || hint.contains("repository") || hint.contains("issue") {
            if let Ok(endpoint) = std::env::var("GITHUB_MCP_ENDPOINT") {
                println!("     🔧 Using GITHUB_MCP_ENDPOINT from environment: {}", endpoint);
                let auth_headers = get_mcp_auth_headers();
                return Some((endpoint, auth_headers, "github".to_string()));
            }
        }
        
        None
    }

    /// Try to discover and install a capability from a real MCP server
    async fn try_real_mcp_discovery(&mut self, server_url: &str, auth_headers: Option<std::collections::HashMap<String, String>>, hint: &str, server_name: &str) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        // Create session manager and initialize session (like single_mcp_discovery.rs)
        let session_manager = MCPSessionManager::new(auth_headers);
        let client_info = MCPServerInfo {
            name: "ccos-autonomous-agent".to_string(),
            version: "1.0.0".to_string(),
        };
        
        let session = session_manager.initialize_session(server_url, &client_info).await
            .map_err(|e| format!("MCP initialization failed: {}", e))?;
        
        // Call tools/list on the session
        let tools_resp = session_manager
            .make_request(&session, "tools/list", serde_json::json!({}))
            .await
            .map_err(|e| format!("MCP tools/list failed: {}", e))?;
        
        // Parse tools array
        let tools_array = tools_resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| "Invalid MCP tools/list response — no tools array")?;
        
        println!("     🔍 Found {} tools from MCP server", tools_array.len());
        
        // Build list of tool candidates with scores using existing helpers
        let mut candidates: Vec<(f64, String, serde_json::Value)> = Vec::new();
        
        for tool_json in tools_array {
            let tool_name = tool_json
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let description = tool_json
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            
            // Use existing scoring helpers from discovery module
            let score = compute_mcp_tool_score(hint, &tool_name, description);
            
            if score > 0.0 {
                candidates.push((score, tool_name, tool_json.clone()));
            }
        }
        
        // Sort by score (descending) and take best match
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        
        // Log top candidates for debugging
        if !candidates.is_empty() {
            println!("     📊 Top MCP tool candidates:");
            for (score, name, _) in candidates.iter().take(3) {
                println!("        - {} (score: {:.2})", name, score);
            }
        }
        
        // Select best match if score is above threshold (>= 3.0 or overlap >= 0.75)
        if let Some((score, tool_name, tool_json)) = candidates.first() {
            let overlap = keyword_overlap(hint, tool_name);
            if *score >= 3.0 || overlap >= 0.75 {
                println!("     ✅ Matched MCP tool: {} (score: {:.2}, overlap: {:.2})", tool_name, score, overlap);
                
                // Convert to DiscoveredMCPTool
                let description = tool_json.get("description").and_then(|d| d.as_str()).map(String::from);
                let input_schema_json = tool_json.get("inputSchema").cloned();
                let input_schema = input_schema_json.as_ref()
                    .and_then(|s| MCPIntrospector::type_expr_from_json_schema(s).ok());
                
                let introspector = MCPIntrospector::new();
                let discovered_tool = DiscoveredMCPTool {
                    tool_name: tool_name.clone(),
                    description: description.clone(),
                    input_schema,
                    output_schema: None,
                    input_schema_json,
                };
                
                // Create capability manifest using the server name from overrides/config
                let introspection_result = ccos::synthesis::mcp_introspector::MCPIntrospectionResult {
                    server_url: server_url.to_string(),
                    server_name: server_name.to_string(),
                    protocol_version: session.protocol_version.clone(),
                    tools: vec![discovered_tool],
                };
                
                let manifest = introspector.create_capability_from_mcp_tool(
                    &introspection_result.tools[0],
                    &introspection_result
                ).map_err(|e| format!("Failed to create manifest: {}", e))?;
                
                // Save discovered MCP capability using MCPIntrospector (like single_mcp_discovery.rs)
                let implementation_code = introspector.generate_mcp_rtfs_implementation(
                    &introspection_result.tools[0],
                    server_url
                );
                let output_dir = get_capabilities_discovered_dir();
                match introspector.save_capability_to_rtfs(
                    &manifest,
                    &implementation_code,
                    &output_dir,
                    None,
                ) {
                    Ok(path) => println!("     💾 Saved discovered MCP capability to: {}", path.display()),
                    Err(e) => eprintln!("     ⚠️  Failed to save MCP capability: {}", e),
                }
                
                return Ok(Some(manifest));
            } else {
                println!("     ⚠️  Best match '{}' below threshold (score: {:.2}, overlap: {:.2})", tool_name, score, overlap);
            }
        }
        
        Ok(None)
    }

    async fn install_generic_mock_capability(&self, id: &str, description: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("     🛠️  Generating generic mock data for '{}'...", id);
        
        let prompt = format!(
            r#"Generate a sample JSON return value for a tool named "{}" which has the description: "{}".
            
            If the tool implies returning a list (e.g. "list_items", "get_records", "search_results"), return a JSON Array with 2-3 realistic items.
            If it implies a single object, return a JSON Object.
            
            Respond ONLY with the JSON.
            "#,
            id, description
        );
        
        let json_str = self.arbiter.generate_raw_text(&prompt).await?;
        let clean_json = extract_json(&json_str);
        
        let sample_data: serde_json::Value = serde_json::from_str(clean_json).unwrap_or(serde_json::json!({"status": "mocked", "message": "LLM failed to generate sample data"}));
        
        let rtfs_value = json_to_rtfs_value(sample_data);
        
        let handler = Arc::new(move |_args: &Value| -> RuntimeResult<Value> {
            Ok(rtfs_value.clone())
        });
        
        self.marketplace.register_local_capability(
            id.to_string(),
            format!("Mock: {}", id),
            description.to_string(),
            handler
        ).await?;
        
        Ok(())
    }

    async fn select_tool_robust(&self, goal: &str, tools: &[String]) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error + Send + Sync>> {
        let prompt = format!(
            r#"You are an expert tool selector.
Goal: "{}"

Available Tools (Format: "ToolID: Description"):
{}

Select the tool ID that BEST matches the goal.
If NONE of the tools are a good match, respond with "NO_MATCH".

If you select a tool, extract the arguments from the goal.
Respond in this JSON format:
{{
  "tool": "ToolID_or_NO_MATCH",
  "arguments": {{ "arg1": "value1" }}
}}
"#,
            goal,
            tools.join("\n")
        );

        let response = self.arbiter.generate_raw_text(&prompt).await?;
        let json_str = extract_json(&response);
        
        #[derive(serde::Deserialize)]
        struct Selection {
            tool: String,
            arguments: Option<HashMap<String, String>>,
        }

        let selection: Selection = serde_json::from_str(json_str)?;
        
        if selection.tool == "NO_MATCH" {
            Ok(None)
        } else {
            Ok(Some((selection.tool, selection.arguments.unwrap_or_default())))
        }
    }

    async fn repair_plan(&self, plan: &str, error: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        println!("     🔧 Plan execution failed. Attempting to repair...");
        
        let prompt = format!(
            r#"The following RTFS plan failed to execute.
Error: {}

Plan:
{}

Please fix the plan to resolve the error.
Respond ONLY with the corrected RTFS code. Do not add markdown formatting or explanation.
"#,
            error, plan
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;
        
        // Clean up code (similar to try_synthesize)
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end+1..];
            } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len()-3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();
        
        if clean_code.starts_with("rtfs") || clean_code.starts_with("lisp") {
            if let Some(space) = clean_code.find(' ') {
                clean_code = &clean_code[space+1..];
            }
        }
        
        Ok(clean_code.trim().to_string())
    }

    fn generate_call(&self, capability_id: &str, args: HashMap<String, String>) -> String {
        let args_str = args.iter()
            .map(|(k, v)| format!(":{} \"{}\"", k, v))
            .collect::<Vec<_>>()
            .join(" ");
        
        format!("(call \"{}\" {{{}}})", capability_id, args_str)
    }
}

// ============================================================================
// Utils
// ============================================================================

/// Convert a RTFS Value to its RTFS literal representation
fn value_to_rtfs_literal(value: &Value) -> String {
    use rtfs::ast::MapKey;
    match value {
        Value::Nil => "nil".to_string(),
        Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"").replace("\n", "\\n")),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 {
                (*f as i64).to_string()
            } else {
                f.to_string()
            }
        }
        Value::Boolean(b) => b.to_string(),
        Value::Vector(v) => {
            let items: Vec<String> = v.iter().map(value_to_rtfs_literal).collect();
            format!("[{}]", items.join(" "))
        }
        Value::Map(m) => {
            let pairs: Vec<String> = m.iter().map(|(k, v)| {
                let key_str = match k {
                    MapKey::String(s) => format!("\"{}\"", s),
                    MapKey::Keyword(kw) => format!(":{}", kw.0),
                    MapKey::Integer(i) => i.to_string(),
                };
                format!("{} {}", key_str, value_to_rtfs_literal(v))
            }).collect();
            format!("{{{}}}", pairs.join(" "))
        }
        Value::List(l) => {
            let items: Vec<String> = l.iter().map(value_to_rtfs_literal).collect();
            format!("({})", items.join(" "))
        }
        Value::Symbol(s) => s.0.clone(),
        Value::Keyword(k) => format!(":{}", k.0),
        Value::Timestamp(t) => format!("\"{}\"", t),
        Value::Uuid(u) => format!("\"{}\"", u),
        Value::ResourceHandle(r) => format!("\"{}\"", r),
        Value::Function(_) => "#<function>".to_string(),
        Value::FunctionPlaceholder(_) => "#<function-placeholder>".to_string(),
        Value::Error(e) => format!("#<error: {}>", e.message),
    }
}

fn extract_json(response: &str) -> &str {
    let response = response.trim();
    
    // Handle code blocks
    let clean = if response.starts_with("```") {
        let mut lines = response.lines();
        lines.next(); // skip start fence
        let content: Vec<&str> = lines.collect();
        let joined = content.join("\n");
        if let Some(end) = joined.rfind("```") {
            joined[..end].to_string()
        } else {
            joined
        }
    } else {
        response.to_string()
    };

    // Simpler approach compatible with &str return:
    let start_idx = response.find(|c| c == '{' || c == '[');
    let end_idx = response.rfind(|c| c == '}' || c == ']');
    
    if let (Some(s), Some(e)) = (start_idx, end_idx) {
        if s <= e {
            return &response[s..=e];
        }
    }
    
    response
}

fn load_agent_config(config_path: &str) -> Result<AgentConfig, Box<dyn Error + Send + Sync>> {
    // Try the provided path first, then try parent directory (for running from ccos/ subdirectory)
    let path = std::path::Path::new(config_path);
    let actual_path = if path.exists() {
        path.to_path_buf()
    } else {
        // Try parent directory
        let parent_path = std::path::Path::new("..").join(config_path);
        if parent_path.exists() {
            parent_path
        } else {
            return Err(format!(
                "Config file not found: '{}' (also tried '../{}'). Run from the workspace root directory.",
                config_path, config_path
            ).into());
        }
    };
    
    let mut content = std::fs::read_to_string(&actual_path)
        .map_err(|e| format!("Failed to read config file '{}': {}", actual_path.display(), e))?;
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    toml::from_str(&content).map_err(|e| format!("failed to parse agent config: {}", e).into())
}

fn apply_llm_profile(agent_config: &AgentConfig, profile: Option<&str>) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(profile_name) = profile {
        let (expanded_profiles, _, _) =
            rtfs::config::profile_selection::expand_profiles(agent_config);

        if let Some(llm_profile) = expanded_profiles.iter().find(|p| p.name == profile_name) {
            std::env::set_var("CCOS_DELEGATING_PROVIDER", llm_profile.provider.clone());
            std::env::set_var("CCOS_DELEGATING_MODEL", llm_profile.model.clone());
            if let Some(api_key_env) = &llm_profile.api_key_env {
                if let Ok(api_key) = std::env::var(api_key_env) {
                    std::env::set_var("OPENAI_API_KEY", api_key);
                }
            } else if let Some(api_key) = &llm_profile.api_key {
                std::env::set_var("OPENAI_API_KEY", api_key.clone());
            }
        } else {
            return Err(format!("LLM profile '{}' not found in config", profile_name).into());
        }
    }
    Ok(())
}

/// Save synthesized capability to a file for inspection
fn save_synthesized_capability(id: &str, code: &str, description: &str, hint: &str) {
    // Determine output directory - save in capabilities/generated/<id>/capability.rtfs
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    // Check if we're in the workspace root (has ccos/Cargo.toml) or inside ccos/ dir
    let base_dir = if root.join("ccos/Cargo.toml").exists() {
        // We're at workspace root, save in capabilities/
        root.join("capabilities/generated")
    } else if root.join("Cargo.toml").exists() && root.ends_with("ccos") {
        // We're inside ccos/ directory, go up to workspace root
        root.parent().unwrap_or(&root).join("capabilities/generated")
    } else {
        // Fallback
        root.join("capabilities/generated")
    };
    
    // Create directory for this capability: capabilities/generated/<id>/
    let cap_dir = base_dir.join(id.replace(".", "_").replace("/", "_"));
    if let Err(e) = std::fs::create_dir_all(&cap_dir) {
        eprintln!("     ⚠️  Failed to create synthesized capability directory: {}", e);
        return;
    }
    
    // Save as capability.rtfs (matching existing structure)
    let filepath = cap_dir.join("capability.rtfs");
    
    // Generate RTFS capability file content
    let rtfs_content = format!(
        r#";; Synthesized Capability: {}
;; Description: {}
;; Hint: {}
;; Generated at: {}

(capability
  :id "{}"
  :name "Synthesized: {}"
  :description "{}"
  :implementation
    {}
)
"#,
        id,
        description,
        hint,
        chrono::Utc::now().to_rfc3339(),
        id,
        hint,
        description.replace("\"", "\\\""),
        code
    );
    
    match std::fs::write(&filepath, &rtfs_content) {
        Ok(_) => println!("     💾 Saved synthesized capability to: {}", filepath.display()),
        Err(e) => eprintln!("     ⚠️  Failed to save synthesized capability: {}", e),
    }
}

/// Get the discovered capabilities directory path
fn get_capabilities_discovered_dir() -> std::path::PathBuf {
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    // Check if we're in the workspace root (has ccos/Cargo.toml) or inside ccos/ dir
    if root.join("ccos/Cargo.toml").exists() {
        // We're at workspace root
        root.join("capabilities/discovered")
    } else if root.join("Cargo.toml").exists() && root.ends_with("ccos") {
        // We're inside ccos/ directory, go up to workspace root
        root.parent().unwrap_or(&root).join("capabilities/discovered")
    } else {
        // Fallback
        root.join("capabilities/discovered")
    }
}

/// Compute MCP tool match score using existing scoring helpers
/// Mirrors the logic from MissingCapabilityResolver::compute_tool_score
fn compute_mcp_tool_score(hint: &str, tool_name: &str, description: &str) -> f64 {
    let mut score = 0.0;
    
    // Use existing description matcher
    score += calculate_description_match_score(hint, description, tool_name);
    
    // Keyword overlap (using tokenize approach from MissingCapabilityResolver)
    let overlap = keyword_overlap(hint, tool_name);
    score += overlap * 2.5;
    
    // Extract last part of hint (e.g., "list" from "github.issues.list")
    let hint_last = hint
        .split('.')
        .last()
        .unwrap_or(hint)
        .to_ascii_lowercase();
    let tool_lower = tool_name.to_ascii_lowercase();
    
    // Exact match bonus
    if tool_lower == hint_last {
        score += 2.0;
    } else if tool_lower.contains(&hint_last) || hint_last.contains(&tool_lower) {
        score += 1.0;
    }
    
    // Normalized hint match
    if hint.replace('.', "_").to_ascii_lowercase().contains(&tool_lower) {
        score += 1.0;
    }
    
    // Action verb synonym matching using existing helper
    let hint_verbs = extract_action_verbs(&hint_last);
    let tool_verbs = extract_action_verbs(&tool_lower);
    let action_verb_score = calculate_action_verb_match_score(&hint_verbs, &tool_verbs);
    if action_verb_score > 0.0 {
        score += action_verb_score * 2.0;
    }
    
    score
}

/// Calculate keyword overlap between two identifiers
/// Mirrors MissingCapabilityResolver::keyword_overlap
fn keyword_overlap(lhs: &str, rhs: &str) -> f64 {
    let lhs_tokens: HashSet<String> = lhs
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect();
    
    let rhs_tokens: HashSet<String> = rhs
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect();
    
    if lhs_tokens.is_empty() || rhs_tokens.is_empty() {
        return 0.0;
    }
    
    let intersection = lhs_tokens.intersection(&rhs_tokens).count();
    if intersection == 0 {
        return 0.0;
    }
    
    intersection as f64 / lhs_tokens.len().max(rhs_tokens.len()) as f64
}

/// Convert serde_json::Value to rtfs::Value
fn json_to_rtfs_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Integer(0)
            }
        },
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(a) => {
            Value::Vector(a.into_iter().map(json_to_rtfs_value).collect())
        },
        serde_json::Value::Object(o) => {
            let mut map = HashMap::new();
            for (k, v) in o {
                map.insert(
                    rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(k)),
                    json_to_rtfs_value(v)
                );
            }
            Value::Map(map)
        }
    }
}

// ============================================================================
// MCP Server Resolution Helpers
// ============================================================================

/// Resolve MCP server URL and name from overrides.json
/// This checks the curated overrides file for matching server configurations
/// Returns (url, server_name) where server_name is derived from the match pattern prefix (e.g., "github" from "github.*")
fn resolve_server_url_from_overrides(hint: &str) -> Option<(String, String)> {
    // Try to load curated overrides from 'capabilities/mcp/overrides.json' (in workspace root)
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    // Check if we're in the workspace root (has ccos/Cargo.toml) or inside ccos/ dir
    let overrides_path = if root.join("ccos/Cargo.toml").exists() {
        // We're at workspace root
        root.join("capabilities/mcp/overrides.json")
    } else if root.join("Cargo.toml").exists() && root.ends_with("ccos") {
        // We're inside ccos/ directory, go up to workspace root
        root.parent().unwrap_or(&root).join("capabilities/mcp/overrides.json")
    } else {
        // Fallback
        root.join("capabilities/mcp/overrides.json")
    };

    if !overrides_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&overrides_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let entries = parsed.get("entries")?.as_array()?;

    for entry in entries {
        if let Some(server) = entry.get("server") {
            // Check server name equality first
            if let Some(name) = server.get("name").and_then(|n| n.as_str()) {
                if hint.contains(name) || name.contains(hint) {
                    // Get best HTTP remote
                    if let Some(url) = get_http_remote_url(server) {
                        // Use the server name from overrides as the namespace
                        return Some((url, name.to_string()));
                    }
                }
            }

            // Check if `matches` patterns include the hint
            if let Some(matches) = entry.get("matches").and_then(|m| m.as_array()) {
                for pat in matches {
                    if let Some(p) = pat.as_str() {
                        // Check if pattern matches hint (pattern may contain wildcards like "github.*")
                        let pattern_clean = p.trim_end_matches(".*").trim_end_matches('*');
                        if hint.contains(pattern_clean) || pattern_clean.contains(hint) {
                            if let Some(url) = get_http_remote_url(server) {
                                // Use the pattern prefix as the server namespace (e.g., "github" from "github.*")
                                let server_name = pattern_clean.to_string();
                                return Some((url, server_name));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Extract HTTP/HTTPS remote URL from server definition
fn get_http_remote_url(server: &serde_json::Value) -> Option<String> {
    if let Some(remotes) = server.get("remotes").and_then(|r| r.as_array()) {
        for remote in remotes {
            if let Some(url) = remote.get("url").and_then(|u| u.as_str()) {
                if url.starts_with("http://") || url.starts_with("https://") {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}

/// Get MCP authentication headers from environment variables
fn get_mcp_auth_headers() -> Option<std::collections::HashMap<String, String>> {
    // Use MCP_AUTH_TOKEN for MCP server authentication
    if let Ok(tok) = std::env::var("MCP_AUTH_TOKEN") {
        if !tok.is_empty() {
            let mut headers = std::collections::HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", tok));
            println!("     🔑 Using auth token from MCP_AUTH_TOKEN");
            return Some(headers);
        }
    }
    
    None
}

// ============================================================================
// Main Entry
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();
    println!("🤖 Autonomous Agent Demo (Iterative)");
    println!("====================================");
    println!("Goal: {}", args.goal);

    // Setup
    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
        println!("⚠️ CCOS_DELEGATING_MODEL not set, defaulting to 'gpt-4o'");
        std::env::set_var("CCOS_DELEGATING_MODEL", "gpt-4o");
    }
    std::env::set_var("CCOS_DELEGATION_ENABLED", "true");

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            Default::default(),
            None,
            Some(agent_config),
            None,
        )
        .await?,
    );

    // Register basic tools
    ccos::capabilities::defaults::register_default_capabilities(&ccos.get_capability_marketplace()).await?;

    // Initialize Planner
    let mut planner = IterativePlanner::new(ccos.clone(), args.simulate_error, args.no_mock)?;

    // Plan
    println!("\n🏗️  Building Plan...");
    let final_plan_rtfs = planner.solve(&args.goal, 0).await?;

    println!("\n📝 Generated RTFS Plan:");
    println!("--------------------------------------------------");
    println!("{}", final_plan_rtfs);
    println!("--------------------------------------------------");

    // Execute
    println!("\n🚀 Executing Plan...");
    
    let mut current_plan_rtfs = final_plan_rtfs;
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 3;
    
    loop {
        attempts += 1;
        let plan_obj = ccos::types::Plan {
            plan_id: "iterative-plan".to_string(),
            name: Some("Generated Plan".to_string()),
            body: ccos::types::PlanBody::Rtfs(current_plan_rtfs.clone()),
            intent_ids: vec![], // Simplification
            ..Default::default()
        };

        let context = RuntimeContext::full();
        let result = ccos.validate_and_execute_plan(plan_obj, &context).await;

        let (success, error_msg) = match result {
            Ok(exec_result) => {
                if exec_result.success {
                    println!("\n🏁 Execution Result:");
                    println!("   Success: {}", exec_result.success);
                    println!("   Result: {:?}", exec_result.value);
                    break;
                } else {
                    let msg = exec_result.metadata.get("error")
                        .map(|v| value_to_rtfs_literal(v))
                        .unwrap_or_else(|| "Unknown error".to_string());
                    (false, msg)
                }
            },
            Err(e) => {
                (false, format!("Runtime Error: {}", e))
            }
        };

        if !success {
            println!("\n❌ Execution Failed (Attempt {}/{}): {}", attempts, MAX_ATTEMPTS, error_msg);
            
            if attempts >= MAX_ATTEMPTS {
                println!("   Giving up after {} attempts.", MAX_ATTEMPTS);
                break;
            }
            
            // Repair
            match planner.repair_plan(&current_plan_rtfs, &error_msg).await {
                Ok(repaired) => {
                    println!("   📝 Repaired Plan:\n{}", repaired);
                    current_plan_rtfs = repaired;
                },
                Err(e) => {
                    println!("   ⚠️ Failed to repair plan: {}", e);
                    break;
                }
            }
        }
    }

    // Dump Trace
    println!("\n🔍 Planning Trace:");
    println!("{}", serde_json::to_string_pretty(&planner.trace)?);
    
    Ok(())
}
