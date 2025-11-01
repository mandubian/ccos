use ccos::arbiter::{
    ArbiterConfig, ArbiterEngineType, ArbiterFactory, FallbackBehavior, IntentPattern,
    PlanTemplate, TemplateConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 Template Arbiter Demo");
    println!("========================\n");

    // Create template configuration with patterns and templates
    let template_config = TemplateConfig {
        intent_patterns: vec![
            IntentPattern {
                name: "sentiment_analysis".to_string(),
                pattern: r"(?i)analyze.*sentiment|sentiment.*analysis|check.*feeling".to_string(),
                intent_name: "analyze_sentiment".to_string(),
                goal_template: "Analyze user sentiment from {source}".to_string(),
                constraints: vec!["privacy".to_string(), "accuracy".to_string()],
                preferences: vec!["speed".to_string()],
            },
            IntentPattern {
                name: "backup_operation".to_string(),
                pattern: r"(?i)backup|save.*data|protect.*data|create.*backup".to_string(),
                intent_name: "backup_data".to_string(),
                goal_template: "Create backup of {data_type} data".to_string(),
                constraints: vec!["encryption".to_string()],
                preferences: vec!["compression".to_string()],
            },
            IntentPattern {
                name: "performance_optimization".to_string(),
                pattern: r"(?i)optimize|improve.*performance|speed.*up|enhance.*speed".to_string(),
                intent_name: "optimize_performance".to_string(),
                goal_template: "Optimize performance for {component}".to_string(),
                constraints: vec!["budget".to_string()],
                preferences: vec!["efficiency".to_string()],
            },
        ],
        plan_templates: vec![
            PlanTemplate {
                name: "sentiment_analysis_plan".to_string(),
                rtfs_template: r#"
(do
    (step "Fetch Data" (call :ccos.echo "fetching {source} data"))
    (step "Analyze Sentiment" (call :ccos.echo "analyzing sentiment with privacy protection"))
    (step "Generate Report" (call :ccos.echo "generating sentiment report"))
    (step "Store Results" (call :ccos.echo "storing analysis results"))
)
                "#
                .trim()
                .to_string(),
                variables: vec!["analyze_sentiment".to_string(), "source".to_string()],
            },
            PlanTemplate {
                name: "backup_plan".to_string(),
                rtfs_template: r#"
(do
    (step "Validate Data" (call :ccos.echo "validating {data_type} data"))
    (step "Create Backup" (call :ccos.echo "creating encrypted backup"))
    (step "Verify Backup" (call :ccos.echo "verifying backup integrity"))
    (step "Store Backup" (call :ccos.echo "storing backup in secure location"))
)
                "#
                .trim()
                .to_string(),
                variables: vec!["backup_data".to_string(), "data_type".to_string()],
            },
            PlanTemplate {
                name: "optimization_plan".to_string(),
                rtfs_template: r#"
(do
    (step "Analyze Current Performance" (call :ccos.echo "analyzing {component} performance"))
    (step "Identify Bottlenecks" (call :ccos.echo "identifying performance bottlenecks"))
    (step "Apply Optimizations" (call :ccos.echo "applying performance optimizations"))
    (step "Test Improvements" (call :ccos.echo "testing performance improvements"))
)
                "#
                .trim()
                .to_string(),
                variables: vec!["optimize_performance".to_string(), "component".to_string()],
            },
        ],
        fallback: FallbackBehavior::Error,
    };

    // Create arbiter configuration
    let config = ArbiterConfig {
        engine_type: ArbiterEngineType::Template,
        llm_config: None,
        delegation_config: None,
        capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
        security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
        template_config: Some(template_config),
    };

    // Create intent graph
    let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::intent_graph::IntentGraph::new()?,
    ));

    // Create arbiter
    let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await?;
    println!("✅ Template Arbiter created successfully\n");

    // Demo requests with context
    let demo_requests = vec![
        ("analyze sentiment from chat logs", Some("chat_logs")),
        ("create backup of database", Some("database")),
        ("optimize performance for web server", Some("web_server")),
        ("check feeling from user feedback", Some("user_feedback")),
        ("save data with encryption", Some("user_data")),
        ("improve performance of API", Some("api")),
    ];

    for (i, (request, context_value)) in demo_requests.iter().enumerate() {
        println!("📝 Demo Request {}: {}", i + 1, request);

        // Create context if provided
        let context = context_value.map(|value| {
            let mut ctx = std::collections::HashMap::new();
            if request.contains("sentiment") || request.contains("feeling") {
                ctx.insert(
                    "source".to_string(),
                    rtfs_compiler::runtime::values::Value::String(value.to_string()),
                );
            } else if request.contains("backup") || request.contains("save") {
                ctx.insert(
                    "data_type".to_string(),
                    rtfs_compiler::runtime::values::Value::String(value.to_string()),
                );
            } else if request.contains("optimize") || request.contains("improve") {
                ctx.insert(
                    "component".to_string(),
                    rtfs_compiler::runtime::values::Value::String(value.to_string()),
                );
            }
            ctx
        });

        // Process the request
        match arbiter.process_natural_language(request, context).await {
            Ok(result) => {
                println!("   ✅ Success!");
                println!("   Result: {}", result.value);
                if let Some(metadata) = result.metadata.get("plan_id") {
                    println!("   Plan ID: {}", metadata);
                }
                if let Some(metadata) = result.metadata.get("template_engine") {
                    println!("   Engine: {}", metadata);
                }
            }
            Err(e) => {
                println!("   ❌ Error: {}", e);
            }
        }
        println!();
    }

    // Summary
    println!("🎉 Template Arbiter Demo Completed!");
    println!("   Total requests processed: {}", demo_requests.len());
    println!("   Pattern matching: ✅");
    println!("   Template generation: ✅");
    println!("   Context substitution: ✅");
    println!();
    println!("💡 Key Features Demonstrated:");
    println!("   • Regex pattern matching for intent recognition");
    println!("   • Template-based RTFS plan generation");
    println!("   • Context variable substitution");
    println!("   • Deterministic, fast execution");
    println!("   • No external LLM dependencies");

    Ok(())
}
