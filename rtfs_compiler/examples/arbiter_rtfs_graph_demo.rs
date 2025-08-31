//! Arbiter RTFS Graph Generation Demo
//!
//! This example showcases the Arbiter's ability to generate a full intent graph
//! from a single high-level goal. The graph itself is generated as an RTFS
//! expression, which is then interpreted to build the structure in the IntentGraph.
//!
//! Note: TUI functionality has been temporarily disabled due to responsiveness issues
//! during orchestration. The demo now runs in text-only mode with detailed progress.

use clap::Parser;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rtfs_compiler::ccos::types::{IntentId, IntentStatus, EdgeType};
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::ccos::arbiter::{
    arbiter_factory::ArbiterFactory,
    arbiter_config::{ArbiterConfig, ArbiterEngineType},
};
use rtfs_compiler::ccos::event_sink::CausalChainIntentEventSink;

#[derive(Parser)]
struct Args {
    #[arg(long, help = "The high-level goal to decompose")]
    goal: Option<String>,
    
    #[arg(long, default_value = "false", help = "Verbose output including plan details")]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();
    
    let goal = args.goal.unwrap_or_else(|| {
        "Create a financial budget for a small business including expense categories, revenue projections, and a monthly cash flow forecast".to_string()
    });
    
    println!("🎯 Goal: {}", goal);
    
    // Create CCOS components
    let causal_chain_inner = CausalChain::new().unwrap();
    let causal_chain = Arc::new(Mutex::new(causal_chain_inner));
    let event_sink = Arc::new(CausalChainIntentEventSink::new(causal_chain.clone()));
    let intent_graph_inner = IntentGraph::with_event_sink(event_sink.clone()).unwrap();
    let intent_graph = Arc::new(Mutex::new(intent_graph_inner));
    let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
    let mut capability_marketplace = CapabilityMarketplace::with_causal_chain(
        capability_registry.clone(),
        Some(causal_chain.clone()),
    );
    // Register default/local capabilities and run discovery
    capability_marketplace.bootstrap().await?;
    let capability_marketplace: Arc<CapabilityMarketplace> = Arc::new(capability_marketplace);
    
    // Configure Arbiter (offline-safe Dummy engine for this demo)
    let arbiter_config = ArbiterConfig {
        engine_type: ArbiterEngineType::Dummy,
        llm_config: None,
        delegation_config: None,
        capability_config: rtfs_compiler::ccos::arbiter::arbiter_config::CapabilityConfig {
            validate_capabilities: true,
            suggest_alternatives: true,
            default_capabilities: vec![],
            marketplace: rtfs_compiler::ccos::arbiter::arbiter_config::CapabilityMarketplaceConfig {
                marketplace_type: rtfs_compiler::ccos::arbiter::arbiter_config::MarketplaceType::Local,
                discovery_endpoints: vec![],
                cache_config: rtfs_compiler::ccos::arbiter::arbiter_config::CacheConfig {
                    enabled: true,
                    ttl_seconds: 3600,
                    max_size: 1000,
                },
            },
        },
        security_config: rtfs_compiler::ccos::arbiter::arbiter_config::SecurityConfig {
            validate_intents: true,
            validate_plans: true,
            max_plan_complexity: 100,
            allowed_capability_prefixes: vec!["ccos.".to_string()],
            blocked_capability_prefixes: vec![],
        },
        template_config: None,
    };
    
    let arbiter = ArbiterFactory::create_arbiter(
        arbiter_config,
        intent_graph.clone(),
        Some(capability_marketplace.clone()),
    ).await?;
    
    let orchestrator = Arc::new(Orchestrator::new(
        causal_chain.clone(),
        intent_graph.clone(),
        capability_marketplace.clone(),
    ));
    
    println!("🚀 Starting arbiter with goal: {}", goal);
    
    // Generate the intent graph
    let root_intent_id = arbiter.natural_language_to_graph(&goal).await?;
    
    println!("✅ Graph generated! Root Intent: {}", root_intent_id);
    print_graph_overview(&intent_graph, &root_intent_id);
    
    // Execute orchestration loop
    let ctx = RuntimeContext::full();
    let mut plans_by_intent: HashMap<IntentId, String> = HashMap::new();
    let mut loop_count = 0;
    const MAX_LOOPS: u32 = 20;
    
    println!("\n🚀 Starting orchestration loop...");
    
    loop {
        if loop_count > MAX_LOOPS {
            println!("⚠️ Max loop count reached. Exiting.");
            break;
        }
        
        let ready_intents = {
            let g = intent_graph.lock().unwrap();
            // Use Active intents as "ready" and filter by dependencies
            let mut all = g.get_active_intents();
            
            // Filter intents based on dependencies
            all.retain(|intent| {
                let edges = g.get_edges_for_intent(&intent.intent_id);
                // All DependsOn edges pointing to this intent must be completed
                for e in &edges {
                    if e.edge_type == EdgeType::DependsOn && e.to == intent.intent_id {
                        if let Some(dep) = g.get_intent(&e.from) {
                            if dep.status != IntentStatus::Completed { return false; }
                        } else { return false; }
                    }
                }
                // All children (IsSubgoalOf edges from child->parent) must be completed
                for e in &edges {
                    if e.edge_type == EdgeType::IsSubgoalOf && e.to == intent.intent_id {
                        if let Some(child) = g.get_intent(&e.from) {
                            if child.status != IntentStatus::Completed { return false; }
                        } else { return false; }
                    }
                }
                true
            });
            
            all
        };
        
        if ready_intents.is_empty() {
            let g = intent_graph.lock().unwrap();
            let root_status = g.get_intent(&root_intent_id).map(|i| i.status).unwrap_or(IntentStatus::Failed);
            if root_status == IntentStatus::Completed || root_status == IntentStatus::Failed {
                println!("🏁 Orchestration complete. Root intent status: {:?}", root_status);
                break;
            } else if loop_count > 0 && g.get_active_intents().is_empty() {
                println!("🛑 No active or ready intents. Halting.");
                break;
            }
        } else {
            // Pick the first ready intent and process it
            if let Some(intent) = ready_intents.into_iter().next() {
                println!("\n📋 Processing intent {} (Loop {})", loop_count + 1, loop_count + 1);
                println!("   Intent: {} (Goal: \"{}\")", intent.intent_id, intent.goal);
                println!("   ⚙️  Generating plan...");

                let plan_result = arbiter.generate_plan_for_intent(&intent).await?;
                if let rtfs_compiler::ccos::types::PlanBody::Rtfs(code) = &plan_result.plan.body {
                    plans_by_intent.insert(intent.intent_id.clone(), code.clone());
                    if args.verbose { println!("   📄 Plan: {}", code); }
                }

                println!("   ⚡ Executing plan...");
                let _ = orchestrator.execute_plan(&plan_result.plan, &ctx).await;
                println!("   ✅ Plan execution completed");
            }
        }
        loop_count += 1;
    }
    
    println!("\n📊 Final Graph State:");
    print_graph_overview(&intent_graph, &root_intent_id);
    
    println!("\n🗺️  Complete Graph with Plans:");
    print_graph_with_plans(&intent_graph, &root_intent_id, &plans_by_intent);
    
    println!("\n🎉 Demo completed successfully!");
    
    Ok(())
}

fn print_graph_overview(intent_graph: &Arc<Mutex<IntentGraph>>, root_intent_id: &IntentId) {
    let g = intent_graph.lock().unwrap();
    
    println!("\n📈 Intent Graph Overview:");
    
    fn recurse(g: &IntentGraph, current: &IntentId, depth: usize, seen: &mut std::collections::HashSet<IntentId>) {
        if seen.contains(current) { return; }
        seen.insert(current.clone());
        
        if let Some(intent) = g.get_intent(current) {
            let indent = "  ".repeat(depth);
            let status_emoji = match intent.status {
                IntentStatus::Active => "🟡",
                IntentStatus::Executing => "🔵", 
                IntentStatus::Completed => "✅",
                IntentStatus::Failed => "❌",
                IntentStatus::Archived => "📦",
                IntentStatus::Suspended => "⏸️",
            };
            let name = intent.name.clone().unwrap_or_else(|| "<unnamed>".to_string());
            println!("{}{}[{:?}] {} — {}", indent, status_emoji, intent.status, name, intent.goal);
            
            for child in g.get_child_intents(&intent.intent_id) {
                recurse(g, &child.intent_id, depth + 1, seen);
            }
        }
    }
    
    let mut seen = std::collections::HashSet::new();
    recurse(&g, root_intent_id, 0, &mut seen);
}

fn print_graph_with_plans(
    intent_graph: &Arc<Mutex<IntentGraph>>,
    root_intent_id: &IntentId,
    plans_by_intent: &HashMap<IntentId, String>,
) {
    let g = intent_graph.lock().unwrap();
    
    fn recurse(g: &IntentGraph, current: &IntentId, plans: &HashMap<IntentId, String>, depth: usize, seen: &mut std::collections::HashSet<IntentId>) {
        if seen.contains(current) { return; }
        seen.insert(current.clone());
        
        if let Some(intent) = g.get_intent(current) {
            let indent = "  ".repeat(depth);
            let status_emoji = match intent.status {
                IntentStatus::Active => "🟡",
                IntentStatus::Executing => "🔵",
                IntentStatus::Completed => "✅", 
                IntentStatus::Failed => "❌",
                IntentStatus::Archived => "📦",
                IntentStatus::Suspended => "⏸️",
            };
            let name = intent.name.clone().unwrap_or_else(|| "<unnamed>".to_string());
            println!("{}{}[{:?}] {} — {}", indent, status_emoji, intent.status, name, intent.goal);
            
            if let Some(plan) = plans.get(&intent.intent_id) {
                let steps = extract_steps_from_plan(plan);
                if !steps.is_empty() {
                    println!("{}  📋 Plan Steps:", indent);
                    for (i, step) in steps.iter().enumerate() {
                        println!("{}    {}. {}", indent, i + 1, step);
                    }
                } else {
                    let plan_preview = if plan.len() > 80 {
                        format!("{}...", &plan[..80])
                    } else {
                        plan.clone()
                    };
                    println!("{}  📄 Plan: {}", indent, plan_preview);
                }
            }
            
            for child in g.get_child_intents(&intent.intent_id) {
                recurse(g, &child.intent_id, plans, depth + 1, seen);
            }
        }
    }
    
    let mut seen = std::collections::HashSet::new();
    recurse(&g, root_intent_id, plans_by_intent, 0, &mut seen);
}

fn extract_steps_from_plan(plan: &str) -> Vec<String> {
    let mut steps = Vec::new();
    
    // Look for (step "description" ...) patterns
    let mut current_pos = 0;
    while let Some(step_pos) = plan[current_pos..].find("(step ") {
        let absolute_pos = current_pos + step_pos;
        current_pos = absolute_pos + 6; // Move past "(step "
        
        if let Some(quote_start) = plan[current_pos..].find('"') {
            current_pos += quote_start + 1;
            if let Some(quote_end) = plan[current_pos..].find('"') {
                let step_desc = plan[current_pos..current_pos + quote_end].to_string();
                steps.push(step_desc);
                current_pos += quote_end + 1;
            }
        }
    }
    
    steps
}
