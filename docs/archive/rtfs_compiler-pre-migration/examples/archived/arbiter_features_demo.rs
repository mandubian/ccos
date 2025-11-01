use rtfs_compiler::ccos::arbiter::arbiter_config::{
    AgentDefinition, AgentRegistryConfig, RegistryType,
};
use rtfs_compiler::ccos::arbiter::{
    ArbiterConfig, ArbiterEngineType, ArbiterFactory, DelegationConfig, FallbackBehavior,
    IntentPattern, LlmConfig, LlmProviderType, PlanTemplate, TemplateConfig,
};
use rtfs_compiler::ccos::delegation_keys::{agent, generation};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 CCOS Arbiter Features Demo");
    println!("=============================\n");

    // Demo 1: Template Arbiter
    demo_template_arbiter().await?;

    // Demo 2: Hybrid Arbiter
    demo_hybrid_arbiter().await?;

    // Demo 3: Delegating Arbiter
    demo_delegating_arbiter().await?;

    // Demo 4: LLM Arbiter
    demo_llm_arbiter().await?;

    println!("🎉 All Arbiter Features Demo Completed!");
    println!("   ✅ Template Arbiter: Pattern matching and templates");
    println!("   ✅ Hybrid Arbiter: Template + LLM fallback");
    println!("   ✅ Delegating Arbiter: LLM + agent delegation");
    println!("   ✅ LLM Arbiter: Pure LLM-driven reasoning");
    println!();
    println!("💡 Key Features Demonstrated:");
    println!("   • Multiple engine types with different capabilities");
    println!("   • Configuration-driven architecture");
    println!("   • Agent delegation and registry");
    println!("   • Fallback strategies");
    println!("   • Context-aware processing");
    println!("   • RTFS plan generation");

    Ok(())
}

async fn demo_template_arbiter() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔧 Demo 1: Template Arbiter");
    println!("---------------------------");

    // Create template configuration
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

    // Demo requests
    let demo_requests = vec![
        ("analyze sentiment from chat logs", Some("chat_logs")),
        ("create backup of database", Some("database")),
        ("optimize performance for web server", Some("web_server")),
    ];

    for (i, (request, context_value)) in demo_requests.iter().enumerate() {
        println!("📝 Request {}: {}", i + 1, request);

        // Create context if provided
        let context = context_value.map(|value| {
            let mut ctx = std::collections::HashMap::new();
            if request.contains("sentiment") {
                ctx.insert(
                    "source".to_string(),
                    rtfs_compiler::runtime::values::Value::String(value.to_string()),
                );
            } else if request.contains("backup") {
                ctx.insert(
                    "data_type".to_string(),
                    rtfs_compiler::runtime::values::Value::String(value.to_string()),
                );
            } else if request.contains("optimize") {
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

    println!("✅ Template Arbiter Demo Completed!\n");
    Ok(())
}

async fn demo_hybrid_arbiter() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Demo 2: Hybrid Arbiter");
    println!("-------------------------");

    // Create template configuration
    let template_config = TemplateConfig {
        intent_patterns: vec![IntentPattern {
            name: "sentiment_analysis".to_string(),
            pattern: r"(?i)analyze.*sentiment|sentiment.*analysis".to_string(),
            intent_name: "analyze_sentiment".to_string(),
            goal_template: "Analyze user sentiment from {source}".to_string(),
            constraints: vec!["privacy".to_string(), "accuracy".to_string()],
            preferences: vec!["speed".to_string()],
        }],
        plan_templates: vec![PlanTemplate {
            name: "sentiment_analysis_plan".to_string(),
            rtfs_template: r#"
(do
    (step "Fetch Data" (call :ccos.echo "fetching {source} data"))
    (step "Analyze Sentiment" (call :ccos.echo "analyzing sentiment"))
    (step "Generate Report" (call :ccos.echo "generating sentiment report"))
)
                "#
            .trim()
            .to_string(),
            variables: vec!["analyze_sentiment".to_string(), "source".to_string()],
        }],
        fallback: FallbackBehavior::Llm,
    };

    // Create LLM configuration
    let llm_config = LlmConfig {
        provider_type: LlmProviderType::Stub,
        model: "stub-model".to_string(),
        api_key: None,
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        prompts: Some(rtfs_compiler::ccos::arbiter::prompt::PromptConfig::default()),
        retry_config: rtfs_compiler::ccos::arbiter::arbiter_config::RetryConfig::default(),
        timeout_seconds: Some(30),
    };

    // Create arbiter configuration
    let config = ArbiterConfig {
        engine_type: ArbiterEngineType::Hybrid,
        llm_config: Some(llm_config),
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
    println!("✅ Hybrid Arbiter created successfully\n");

    // Demo requests - mix of template matches and LLM fallback
    let demo_requests = vec![
        ("analyze sentiment from chat logs", Some("chat_logs")), // Template match
        ("create a complex data analysis pipeline", None),       // LLM fallback
        ("optimize database queries for better performance", None), // LLM fallback
    ];

    for (i, (request, context_value)) in demo_requests.iter().enumerate() {
        println!("📝 Request {}: {}", i + 1, request);

        // Create context if provided
        let context = context_value.map(|value| {
            let mut ctx = std::collections::HashMap::new();
            if request.contains("sentiment") {
                ctx.insert(
                    "source".to_string(),
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
                if let Some(metadata) = result.metadata.get("hybrid_engine") {
                    println!("   Engine: {}", metadata);
                }
                if let Some(metadata) = result.metadata.get(generation::GENERATION_METHOD) {
                    println!("   Method: {}", metadata);
                }
            }
            Err(e) => {
                println!("   ❌ Error: {}", e);
            }
        }
        println!();
    }

    println!("✅ Hybrid Arbiter Demo Completed!\n");
    Ok(())
}

async fn demo_delegating_arbiter() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤝 Demo 3: Delegating Arbiter");
    println!("-----------------------------");

    // Create LLM configuration
    let llm_config = LlmConfig {
        provider_type: LlmProviderType::Stub,
        model: "stub-model".to_string(),
        api_key: None,
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        prompts: Some(rtfs_compiler::ccos::arbiter::prompt::PromptConfig::default()),
        retry_config: rtfs_compiler::ccos::arbiter::arbiter_config::RetryConfig::default(),
        timeout_seconds: Some(30),
    };

    // Create delegation configuration with agents
    let delegation_config = DelegationConfig {
        enabled: true,
        threshold: 0.65,
        adaptive_threshold: None,
        max_candidates: 3,
        min_skill_hits: Some(1),
        agent_registry: AgentRegistryConfig {
            registry_type: RegistryType::InMemory,
            database_url: None,
            agents: vec![
                AgentDefinition {
                    agent_id: "sentiment_agent".to_string(),
                    name: "Sentiment Analysis Agent".to_string(),
                    capabilities: vec![
                        "sentiment_analysis".to_string(),
                        "text_processing".to_string(),
                    ],
                    cost: 0.1,
                    trust_score: 0.9,
                    metadata: std::collections::HashMap::<String, String>::new(),
                },
                AgentDefinition {
                    agent_id: "backup_agent".to_string(),
                    name: "Backup Agent".to_string(),
                    capabilities: vec!["backup".to_string(), "encryption".to_string()],
                    cost: 0.2,
                    trust_score: 0.8,
                    metadata: std::collections::HashMap::<String, String>::new(),
                },
                AgentDefinition {
                    agent_id: "optimization_agent".to_string(),
                    name: "Performance Optimization Agent".to_string(),
                    capabilities: vec![
                        "performance_optimization".to_string(),
                        "monitoring".to_string(),
                    ],
                    cost: 0.15,
                    trust_score: 0.85,
                    metadata: std::collections::HashMap::<String, String>::new(),
                },
            ],
        },
            print_extracted_intent: None,
            print_extracted_plan: None,
    };

    // Create arbiter configuration
    let config = ArbiterConfig {
        engine_type: ArbiterEngineType::Delegating,
        llm_config: Some(llm_config),
        delegation_config: Some(delegation_config),
        capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
        security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
        template_config: None,
    };

    // Create intent graph
    let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::intent_graph::IntentGraph::new()?,
    ));

    // Create arbiter
    let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await?;
    println!("✅ Delegating Arbiter created successfully\n");

    // Demo requests that might trigger delegation
    let demo_requests = vec![
        (
            "analyze sentiment from user feedback and provide detailed insights",
            None,
        ),
        (
            "create a comprehensive backup strategy for our production database",
            None,
        ),
        (
            "optimize our web application performance and provide recommendations",
            None,
        ),
        ("simple echo test", None), // Should not trigger delegation
    ];

    for (i, (request, context_value)) in demo_requests.iter().enumerate() {
        println!("📝 Request {}: {}", i + 1, request);

        // Create context if provided
        let context = context_value.map(|value: &str| {
            let mut ctx = std::collections::HashMap::new();
            ctx.insert(
                "context".to_string(),
                rtfs_compiler::runtime::values::Value::String(value.to_string()),
            );
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
                if let Some(metadata) = result.metadata.get("delegating_engine") {
                    println!("   Engine: {}", metadata);
                }
                if let Some(metadata) = result.metadata.get(generation::GENERATION_METHOD) {
                    println!("   Method: {}", metadata);
                }
                if let Some(metadata) = result.metadata.get(agent::DELEGATED_AGENT) {
                    println!("   Delegated Agent: {}", metadata);
                }
            }
            Err(e) => {
                println!("   ❌ Error: {}", e);
            }
        }
        println!();
    }

    println!("✅ Delegating Arbiter Demo Completed!\n");
    Ok(())
}

async fn demo_llm_arbiter() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧠 Demo 4: LLM Arbiter");
    println!("----------------------");

    // Create LLM configuration
    let llm_config = LlmConfig {
        provider_type: LlmProviderType::Stub,
        model: "stub-model".to_string(),
        api_key: None,
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        prompts: Some(rtfs_compiler::ccos::arbiter::prompt::PromptConfig::default()),
        retry_config: rtfs_compiler::ccos::arbiter::arbiter_config::RetryConfig::default(),
        timeout_seconds: Some(30),
    };

    // Create arbiter configuration
    let config = ArbiterConfig {
        engine_type: ArbiterEngineType::Llm,
        llm_config: Some(llm_config),
        delegation_config: None,
        capability_config: rtfs_compiler::ccos::arbiter::CapabilityConfig::default(),
        security_config: rtfs_compiler::ccos::arbiter::SecurityConfig::default(),
        template_config: None,
    };

    // Create intent graph
    let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::intent_graph::IntentGraph::new()?,
    ));

    // Create arbiter
    let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await?;
    println!("✅ LLM Arbiter created successfully\n");

    // Demo requests for pure LLM reasoning
    let demo_requests = vec![
        (
            "create a machine learning pipeline for customer segmentation",
            None,
        ),
        (
            "design a microservices architecture for an e-commerce platform",
            None,
        ),
        (
            "implement a real-time data processing system with streaming analytics",
            None,
        ),
    ];

    for (i, (request, context_value)) in demo_requests.iter().enumerate() {
        println!("📝 Request {}: {}", i + 1, request);

        // Create context if provided
        let context = context_value.map(|value: &str| {
            let mut ctx = std::collections::HashMap::new();
            ctx.insert(
                "context".to_string(),
                rtfs_compiler::runtime::values::Value::String(value.to_string()),
            );
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
                if let Some(metadata) = result.metadata.get("llm_engine") {
                    println!("   Engine: {}", metadata);
                }
                if let Some(metadata) = result.metadata.get(generation::GENERATION_METHOD) {
                    println!("   Method: {}", metadata);
                }
            }
            Err(e) => {
                println!("   ❌ Error: {}", e);
            }
        }
        println!();
    }

    println!("✅ LLM Arbiter Demo Completed!\n");
    Ok(())
}
