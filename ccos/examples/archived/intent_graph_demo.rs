//! Intent Graph Demo (true multi-intent graph with dependencies)
//!
//! Shows how to:
//! - Create a root intent and spawn child intents (IsSubgoalOf + DependsOn)
//! - Register a network capability that routes through the MicroVM-aware CapabilityRegistry
//! - Execute child intents respecting dependencies with Orchestrator
//! - Emit lifecycle and relationship audit to the Causal Chain
//!
//! Try:
//!   cargo run --example intent_graph_demo -- --verbose
//!   cargo run --example intent_graph_demo -- --debug --verbose

use clap::Parser;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::event_sink::CausalChainIntentEventSink;
use ccos::governance_kernel::GovernanceKernel;
use ccos::intent_graph::IntentGraph;
use ccos::plan_archive::PlanArchive;
use ccos::types::StorableIntent;
use ccos::types::{
    EdgeType, ExecutionResult, IntentId, IntentStatus, Plan, PlanBody, PlanLanguage, PlanStatus,
};
use rtfs::runtime::capabilities::registry::CapabilityRegistry;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

#[derive(Parser, Debug)]
#[command(name = "intent_graph_demo")]
#[command(
    about = "Create and execute a dependency-ordered intent graph with MicroVM-aware network capability"
)]
struct Args {
    /// Verbose output
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Debug mode: extra logs
    #[arg(long, default_value_t = false)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("🔗 Intent Graph Demo\n===================\n");

    if args.debug {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
        std::env::set_var("RTFS_ARBITER_DEBUG", "1");
        println!("🔧 Debug mode enabled");
    }

    // --- Core subsystems ---
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::with_event_sink(Arc::new(
        CausalChainIntentEventSink::new(Arc::clone(&causal_chain)),
    ))?));

    let capability_registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    {
        // Ensure mock MicroVM provider selected so network/file ops are allowed via registry path
        let mut reg = capability_registry.write().await;
        let _ = reg.set_microvm_provider("mock");
    }

    let marketplace = CapabilityMarketplace::with_causal_chain(
        Arc::clone(&capability_registry),
        Some(Arc::clone(&causal_chain)),
    );
    marketplace.bootstrap().await?;
    // Remove the bootstrap manifest for http-fetch to force fallback to the CapabilityRegistry
    // This makes marketplace.execute_capability route to registry.execute_capability_with_microvm()
    let _ = marketplace
        .remove_capability("ccos.network.http-fetch")
        .await;
    let marketplace = Arc::new(marketplace);
    let plan_archive = Arc::new(PlanArchive::new());
    let governance_kernel = Arc::new(GovernanceKernel::new(Arc::clone(&causal_chain)));

    // --- Build a graph ---
    // Root intent
    let mut root = StorableIntent::new("Build a tiny report from an HTTP endpoint".to_string());
    root.name = Some("root-report".to_string());
    {
        let mut g = intent_graph.lock().unwrap();
        g.store_intent(root.clone())?;
    }

    // Subgoal: fetch
    let mut fetch = StorableIntent::new("Fetch data from http://localhost:9999/mock".to_string());
    fetch.name = Some("fetch-data".to_string());
    fetch.parent_intent = Some(root.intent_id.clone());

    // Subgoal: analyze (trivial here)
    let mut analyze = StorableIntent::new("Announce url from fetched data".to_string());
    analyze.name = Some("analyze-data".to_string());
    analyze.parent_intent = Some(root.intent_id.clone());

    // Subgoal: persist (announce)
    let mut announce = StorableIntent::new("Announce summary via echo".to_string());
    announce.name = Some("announce".to_string());
    announce.parent_intent = Some(root.intent_id.clone());

    // Store children and create edges
    {
        let mut g = intent_graph.lock().unwrap();
        g.store_intent(fetch.clone())?;
        g.store_intent(analyze.clone())?;
        g.store_intent(announce.clone())?;
        // A is subgoal of B means edge: from=A, to=B
        g.create_edge(
            fetch.intent_id.clone(),
            root.intent_id.clone(),
            EdgeType::IsSubgoalOf,
        )?;
        if args.verbose {
            println!(
                "  [+] edge {} -({:?})-> {}",
                fetch.intent_id,
                EdgeType::IsSubgoalOf,
                root.intent_id
            );
        }
        g.create_edge(
            analyze.intent_id.clone(),
            root.intent_id.clone(),
            EdgeType::IsSubgoalOf,
        )?;
        if args.verbose {
            println!(
                "  [+] edge {} -({:?})-> {}",
                analyze.intent_id,
                EdgeType::IsSubgoalOf,
                root.intent_id
            );
        }
        g.create_edge(
            announce.intent_id.clone(),
            root.intent_id.clone(),
            EdgeType::IsSubgoalOf,
        )?;
        if args.verbose {
            println!(
                "  [+] edge {} -({:?})-> {}",
                announce.intent_id,
                EdgeType::IsSubgoalOf,
                root.intent_id
            );
        }
        // Dependencies: analyze depends on fetch; announce depends on analyze
        g.create_edge(
            analyze.intent_id.clone(),
            fetch.intent_id.clone(),
            EdgeType::DependsOn,
        )?;
        if args.verbose {
            println!(
                "  [+] edge {} -({:?})-> {}",
                analyze.intent_id,
                EdgeType::DependsOn,
                fetch.intent_id
            );
        }
        g.create_edge(
            announce.intent_id.clone(),
            analyze.intent_id.clone(),
            EdgeType::DependsOn,
        )?;
        if args.verbose {
            println!(
                "  [+] edge {} -({:?})-> {}",
                announce.intent_id,
                EdgeType::DependsOn,
                analyze.intent_id
            );
        }
    }

    // Audit relationships in Causal Chain (optional for visibility)
    {
        let mut chain = causal_chain.lock().unwrap();
        let pid = "intent-graph-demo".to_string();
        let _ = chain.log_intent_relationship_created(
            &pid,
            &root.intent_id,
            &fetch.intent_id,
            &root.intent_id,
            "IsSubgoalOf",
            None,
            None,
        );
        let _ = chain.log_intent_relationship_created(
            &pid,
            &root.intent_id,
            &analyze.intent_id,
            &root.intent_id,
            "IsSubgoalOf",
            None,
            None,
        );
        let _ = chain.log_intent_relationship_created(
            &pid,
            &root.intent_id,
            &announce.intent_id,
            &root.intent_id,
            "IsSubgoalOf",
            None,
            None,
        );
        let _ = chain.log_intent_relationship_created(
            &pid,
            &root.intent_id,
            &analyze.intent_id,
            &fetch.intent_id,
            "DependsOn",
            None,
            None,
        );
        let _ = chain.log_intent_relationship_created(
            &pid,
            &root.intent_id,
            &announce.intent_id,
            &analyze.intent_id,
            "DependsOn",
            None,
            None,
        );
    }

    if args.verbose {
        println!("📌 Root: {}", root.intent_id);
        println!("  ├─ fetch: {} (IsSubgoalOf root)", fetch.intent_id);
        println!("  ├─ analyze: {} (DependsOn fetch)", analyze.intent_id);
        println!("  └─ announce: {} (DependsOn analyze)", announce.intent_id);

        // Print raw edges for visibility
        let g = intent_graph.lock().unwrap();
        let mut all_edges = Vec::new();
        for id in [
            &root.intent_id,
            &fetch.intent_id,
            &analyze.intent_id,
            &announce.intent_id,
        ] {
            for e in g.get_edges_for_intent(id) {
                all_edges.push((e.from.clone(), e.to.clone(), format!("{:?}", e.edge_type)));
            }
        }
        all_edges.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        println!("  ◦ edges:");
        for (from, to, kind) in all_edges {
            println!("    - {} -> {} [{}]", from, to, kind);
        }

        // Show a tiny hierarchy overview
        drop(g); // release lock first
        print_graph_overview(&intent_graph, &root.intent_id);
    }

    // --- Plans ---
    // Fetch plan executes network capability (MicroVM-aware through registry bridge)
    let fetch_plan = Plan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        name: Some("fetch-http".to_string()),
        intent_ids: vec![fetch.intent_id.clone()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs(
            "(do (step \"Fetch\" (call :ccos.network.http-fetch \"http://localhost:9999/mock\")))"
                .to_string(),
        ),
        status: PlanStatus::Draft,
        created_at: chrono::Utc::now().timestamp() as u64,
        metadata: Default::default(),
        input_schema: None,
        output_schema: None,
        policies: Default::default(),
        capabilities_required: vec!["ccos.network.http-fetch".to_string()],
        annotations: Default::default(),
    };

    // Analyze plan is trivial (showcase deterministic step)
    let analyze_plan = Plan::new_with_schemas(
        Some("analyze".to_string()),
        vec![analyze.intent_id.clone()],
        PlanBody::Rtfs(
            "(do (step \"Announce-URL\" (call :ccos.echo {:message \"analysis: ok\"})))"
                .to_string(),
        ),
        None,
        None,
        Default::default(),
        vec!["ccos.echo".to_string()],
        Default::default(),
    );

    // Announce plan
    let announce_plan = Plan::new_with_schemas(
        Some("announce".to_string()),
        vec![announce.intent_id.clone()],
        PlanBody::Rtfs(
            "(do (step \"Announce\" (call :ccos.echo {:message \"Done\"})))".to_string(),
        ),
        None,
        None,
        Default::default(),
        vec!["ccos.echo".to_string()],
        Default::default(),
    );

    // Execute respecting dependencies: fetch -> analyze -> announce
    let ctx = RuntimeContext::full();
    execute_intent_with_plan(
        &intent_graph,
        &causal_chain,
        &governance_kernel,
        &fetch,
        &fetch_plan,
        &ctx,
        args.verbose,
    )
    .await;

    // Gate on DependsOn: analyze requires fetch Completed
    if deps_completed(&intent_graph, &analyze.intent_id) {
        execute_intent_with_plan(
            &intent_graph,
            &causal_chain,
            &governance_kernel,
            &analyze,
            &analyze_plan,
            &ctx,
            args.verbose,
        )
        .await;
    } else {
        println!("⚠️  Skipping analyze; dependencies not completed");
    }

    // Gate on DependsOn: announce requires analyze Completed
    if deps_completed(&intent_graph, &announce.intent_id) {
        execute_intent_with_plan(
            &intent_graph,
            &causal_chain,
            &governance_kernel,
            &announce,
            &announce_plan,
            &ctx,
            args.verbose,
        )
        .await;
    } else {
        println!("⚠️  Skipping announce; dependencies not completed");
    }

    // If all subgoals completed, mark root Completed
    if all_subgoals_completed(&intent_graph, &root.intent_id) {
        let mut g = intent_graph.lock().unwrap();
        let _ = g.set_intent_status_with_audit(
            &root.intent_id,
            IntentStatus::Completed,
            Some("intent-graph-demo"),
            None,
        );
    }

    // Render a quick summary
    println!("\n📊 Summary:");
    for (label, intent) in [
        ("root", &root),
        ("fetch", &fetch),
        ("analyze", &analyze),
        ("announce", &announce),
    ] {
        let g = intent_graph.lock().unwrap();
        let status = format!(
            "{:?}",
            g.get_intent(&intent.intent_id)
                .map(|i| i.status)
                .unwrap_or(IntentStatus::Failed)
        );
        println!("  • {} {} -> status {}", label, intent.intent_id, status);
    }

    println!("\n✨ Done.");
    Ok(())
}

// Check if all DependsOn parents of intent are Completed
fn deps_completed(intent_graph: &Arc<Mutex<IntentGraph>>, intent_id: &IntentId) -> bool {
    let g = intent_graph.lock().unwrap();
    let edges = g.get_edges_for_intent(intent_id);
    // For DependsOn, edge direction is from child to parent prerequisite
    let parents: Vec<IntentId> = edges
        .into_iter()
        .filter(|e| e.edge_type == EdgeType::DependsOn && e.from == *intent_id)
        .map(|e| e.to)
        .collect();
    for pid in parents {
        if let Some(p) = g.get_intent(&pid) {
            if p.status != IntentStatus::Completed {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}

async fn execute_intent_with_plan(
    intent_graph: &Arc<Mutex<IntentGraph>>,
    causal_chain: &Arc<Mutex<CausalChain>>,
    governance_kernel: &Arc<GovernanceKernel>,
    storable_intent: &StorableIntent,
    plan: &Plan,
    runtime_context: &RuntimeContext,
    verbose: bool,
) {
    if verbose {
        if let PlanBody::Rtfs(code) = &plan.body {
            println!("\n📄 Plan {} body:\n{}\n", plan.plan_id, code);
        }
    }

    // Audit: mark Executing
    {
        let mut g = intent_graph.lock().unwrap();
        let _ = g.set_intent_status_with_audit(
            &storable_intent.intent_id,
            IntentStatus::Executing,
            Some(&plan.plan_id),
            None,
        );
    }
    {
        let mut chain = causal_chain.lock().unwrap();
        let _ = chain.log_intent_status_change(
            &plan.plan_id,
            &storable_intent.intent_id,
            "Active",
            "Executing",
            "demo:start",
            None,
        );
    }

    // Execute through governance-enforced interface
    let exec = governance_kernel.execute_plan_governed(plan, runtime_context).await;

    // Update status + audit
    match exec {
        Ok(result) => {
            if verbose {
                println!(
                    "✅ {} -> {}",
                    storable_intent.intent_id,
                    render_value(&result.value)
                );
            }
            let mut g = intent_graph.lock().unwrap();
            let current = g
                .get_intent(&storable_intent.intent_id)
                .expect("intent present");
            let _ = g.update_intent_with_audit(
                current,
                &ExecutionResult {
                    success: true,
                    value: result.value.clone(),
                    metadata: Default::default(),
                },
                Some(&plan.plan_id),
                None,
            );
        }
        Err(e) => {
            println!("❌ {} failed: {}", storable_intent.intent_id, e);
            let mut g = intent_graph.lock().unwrap();
            let current = g
                .get_intent(&storable_intent.intent_id)
                .expect("intent present");
            let _ = g.update_intent_with_audit(
                current,
                &ExecutionResult {
                    success: false,
                    value: Value::Nil,
                    metadata: Default::default(),
                }
                .with_error(&e.to_string()),
                Some(&plan.plan_id),
                None,
            );
        }
    }
}

fn render_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Vector(xs) => format!(
            "[{}]",
            xs.iter().map(render_value).collect::<Vec<_>>().join(", ")
        ),
        Value::List(xs) => format!(
            "({})",
            xs.iter().map(render_value).collect::<Vec<_>>().join(", ")
        ),
        Value::Map(m) => {
            let mut items: Vec<(String, &Value)> = m
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        rtfs::ast::MapKey::Keyword(k) => format!(":{}", k.0),
                        rtfs::ast::MapKey::String(s) => format!("\"{}\"", s),
                        rtfs::ast::MapKey::Integer(i) => i.to_string(),
                    };
                    (key_str, v)
                })
                .collect();
            items.sort_by(|a, b| a.0.cmp(&b.0));
            let body = items
                .into_iter()
                .map(|(k, v)| format!("{} {}", k, render_value(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{}}}", body)
        }
        Value::Function(_) => "#<fn>".into(),
        Value::FunctionPlaceholder(_) => "#<fn-p>".into(),
        Value::Error(e) => format!("#<error:{}>", e.message),
        Value::Timestamp(ts) => format!("#timestamp(\"{}\")", ts),
        Value::Uuid(u) => format!("#uuid(\"{}\")", u),
        Value::ResourceHandle(r) => format!("#resource(\"{}\")", r),
        // Value::Atom removed - use host capabilities for state instead
        Value::Symbol(s) => s.0.clone(),
        Value::Keyword(k) => format!(":{}", k.0),
    }
}

// Return true if every IsSubgoalOf child of root has Completed status
fn all_subgoals_completed(intent_graph: &Arc<Mutex<IntentGraph>>, root_id: &IntentId) -> bool {
    let g = intent_graph.lock().unwrap();
    let edges = g.get_edges_for_intent(root_id);
    // children are edges where from is child and to is root with IsSubgoalOf
    let mut has_child = false;
    for e in edges {
        if e.edge_type == EdgeType::IsSubgoalOf && e.to == *root_id {
            has_child = true;
            if let Some(child) = g.get_intent(&e.from) {
                if child.status != IntentStatus::Completed {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    // If there are no children, don't flip to Completed here
    has_child
}

// Simple overview: print parents and children reachable from a root
fn print_graph_overview(intent_graph: &Arc<Mutex<IntentGraph>>, root_id: &IntentId) {
    let g = intent_graph.lock().unwrap();
    println!("\n🧭 Graph overview from root {}:", root_id);
    // List children (direct)
    let children = g.get_child_intents(root_id);
    for c in &children {
        println!("  • child {} [{}]", c.intent_id, format!("{:?}", c.status));
    }

    // For each child, list its DependsOn parents
    for c in &children {
        let edges = g.get_edges_for_intent(&c.intent_id);
        let deps: Vec<_> = edges
            .into_iter()
            .filter(|e| e.edge_type == EdgeType::DependsOn && e.from == c.intent_id)
            .map(|e| e.to)
            .collect();
        if !deps.is_empty() {
            for d in deps {
                if let Some(pi) = g.get_intent(&d) {
                    println!(
                        "     ↳ depends on {} [{}]",
                        pi.intent_id,
                        format!("{:?}", pi.status)
                    );
                } else {
                    println!("     ↳ depends on {} [missing]", d);
                }
            }
        }
    }
}
