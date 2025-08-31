// Demonstrate intent->plan failure when no template produces a capability for the goal
// Run: cargo run --example intent_to_plan_no_capability_demo --manifest-path rtfs_compiler/Cargo.toml

use rtfs_compiler::ccos::arbiter::{TemplateArbiter};
use rtfs_compiler::ccos::arbiter::arbiter_config::{TemplateConfig, IntentPattern, PlanTemplate, FallbackBehavior};
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use rtfs_compiler::ccos::arbiter::arbiter_engine::ArbiterEngine;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use rtfs_compiler::runtime::values::Value;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Build a TemplateArbiter with an intent pattern but deliberately no matching plan template
    let config = TemplateConfig {
        intent_patterns: vec![IntentPattern {
            name: "export_report".to_string(),
            pattern: r"(?i)export.*report".to_string(),
            intent_name: "export_report".to_string(),
            goal_template: "Export quarterly report to PDF".to_string(),
            constraints: vec![],
            preferences: vec![],
        }],
        // Intentionally provide a template unrelated to the intent name to force failure
        plan_templates: vec![PlanTemplate {
            name: "unrelated_plan".to_string(),
            rtfs_template: r#"(do (step \"noop\" (call :ccos.echo \"noop\")))"#.to_string(),
            variables: vec!["some_other_intent".to_string()],
        }],
        fallback: FallbackBehavior::Error,
    };

    let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
    let arbiter = TemplateArbiter::new(config, intent_graph).expect("template arbiter init");

    // Create an intent via the template pattern
    let mut ctx: HashMap<String, Value> = HashMap::new();
    ctx.insert("format".into(), Value::String("pdf".into()));
    let intent = match arbiter.natural_language_to_intent("Please export the report", Some(ctx)).await {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Unexpected: failed to create intent: {e}");
            return;
        }
    };

    // Try to convert intent -> plan; expect an error: "No plan template found for intent: 'export_report'"
    match arbiter.intent_to_plan(&intent).await {
        Ok(plan) => {
            println!("Unexpected: got a plan: {:?}", plan.name);
        }
        Err(e) => {
            println!("Intent→Plan error (as expected): {}", e);
            println!("This shows the path where a goal has no capability/template mapping.");
        }
    }
}
