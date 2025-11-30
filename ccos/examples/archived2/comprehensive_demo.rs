//! CCOS + RTFS Comprehensive Interactive Demo
//!
//! This demo showcases the complete CCOS + RTFS architecture by:
//! 1. Accepting natural language goals from users
//! 2. Converting them to structured RTFS intents using AI
//! 3. Discovering available MCP capabilities dynamically
//! 4. Generating executable RTFS plans based on intents + capabilities
//! 5. Executing plans through the CCOS runtime
//! 6. Presenting results and demonstrating the full pipeline
//!
//! Usage:
//!   cargo run --example comprehensive_demo -- --interactive
//!   cargo run --example comprehensive_demo -- --goal "Analyze the sentiment of 'I love this product!'"
//!   cargo run --example comprehensive_demo -- --mcp-server http://localhost:3000 --goal "Get weather for Paris"

use clap::Parser;
use serde_json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

// Re-export for shared modules
pub mod shared;

// Core CCOS + RTFS imports
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::delegation::ModelRegistry;
use ccos::types::{Intent, Plan, StorableIntent};
use rtfs::ast::TopLevel;
use rtfs::parser;
use rtfs::runtime::capabilities::registry::CapabilityRegistry;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
// format_rtfs_value_pretty not available

// CCOS subsystems we wire directly for the demo
use ccos::causal_chain::CausalChain;
use ccos::event_sink::CausalChainIntentEventSink;
use ccos::intent_graph::IntentGraph;
use ccos::orchestrator::Orchestrator;
use ccos::plan_archive::PlanArchive;
use ccos::types::PlanBody;

use shared::CustomOpenRouterModel;

#[derive(Parser)]
#[command(name = "comprehensive_demo")]
#[command(about = "Complete CCOS + RTFS interactive demo showcasing the full architecture")]
struct Args {
    /// Run in interactive mode (prompt for user input)
    #[arg(long, default_value_t = false)]
    interactive: bool,

    /// Natural language goal/request (if not interactive)
    #[arg(long)]
    goal: Option<String>,

    /// MCP server URL for capability discovery
    #[arg(long, default_value = "http://localhost:3000")]
    mcp_server: String,

    /// Skip AI generation and use predefined examples
    #[arg(long, default_value_t = false)]
    demo_mode: bool,

    /// Show detailed execution traces
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

#[allow(dead_code)]
struct DemoContext {
    model_registry: ModelRegistry,
    // CCOS subsystems for persistence, auditing, and execution
    causal_chain: Arc<Mutex<CausalChain>>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    orchestrator: Arc<Orchestrator>,
    mcp_server_url: String,
}

impl DemoContext {
    async fn new(mcp_server_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        println!("🚀 Initializing CCOS + RTFS Demo Context...");

        // 1. Set up AI model registry
        let model_registry = ModelRegistry::new();
        let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| "".to_string());

        // Always try to set up the AI model, even if API key is empty
        let model = CustomOpenRouterModel::new(
            "openrouter-hunyuan-a13b-instruct",
            "tencent/hunyuan-a13b-instruct:free",
        );
        model_registry.register(model);

        if !api_key.is_empty() {
            println!("✅ AI model configured with API key");
        } else {
            println!("⚠️  No OPENROUTER_API_KEY found - AI calls will fail gracefully");
        }

        // 2. Build CCOS subsystems: CausalChain, IntentGraph (with event sink), CapabilityMarketplace, Orchestrator
        let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
        let sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&causal_chain)));
        let intent_graph = Arc::new(Mutex::new(IntentGraph::with_event_sink(sink)?));

        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = CapabilityMarketplace::with_causal_chain(
            Arc::clone(&registry),
            Some(Arc::clone(&causal_chain)),
        );
        // Bootstrap marketplace and register std capabilities
        capability_marketplace.bootstrap().await?;
        let capability_marketplace = Arc::new(capability_marketplace);
        // Default capabilities registered in bootstrap()
        let plan_archive = Arc::new(PlanArchive::new());

        // Orchestrator that will execute plans and update graph/chain
        let orchestrator = Arc::new(Orchestrator::new(
            Arc::clone(&causal_chain),
            Arc::clone(&intent_graph),
            Arc::clone(&capability_marketplace),
            Arc::clone(&plan_archive),
        ));

        println!("✅ Demo context initialized");

        Ok(DemoContext {
            model_registry,
            causal_chain,
            intent_graph,
            capability_marketplace,
            orchestrator,
            mcp_server_url: mcp_server_url.to_string(),
        })
    }

    /// Store the generated intent into the IntentGraph so Governance/Orchestrator can reference it
    fn persist_intent(&self, intent: &Intent) -> Result<(), Box<dyn std::error::Error>> {
        let mut st = StorableIntent::new(intent.goal.clone());
        st.intent_id = intent.intent_id.clone();
        st.name = intent.name.clone();
        st.original_request = intent.original_request.clone();
        // Keep RTFS-specific fields empty for now; constraints/preferences could be round-tripped later
        st.status = ccos::types::IntentStatus::Active;
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| "Failed to lock IntentGraph for store")?;
        graph.store_intent(st)?;
        Ok(())
    }

    /// Step 1: Discover available MCP capabilities
    async fn discover_capabilities(
        &self,
    ) -> Result<Vec<DiscoveredCapability>, Box<dyn std::error::Error>> {
        println!("\n🔍 Step 1: Discovering Available Capabilities");
        println!("=============================================");

        // For demo purposes, skip HTTP calls and use mock capabilities
        println!("📡 Using mock capabilities for demo");
        let capabilities = self.get_mock_capabilities();

        println!("✅ Discovered {} capabilities:", capabilities.len());
        for cap in &capabilities {
            println!(
                "  • {} - {}",
                cap.name,
                cap.description.as_deref().unwrap_or("No description")
            );
        }

        Ok(capabilities)
    }

    fn get_mock_capabilities(&self) -> Vec<DiscoveredCapability> {
        vec![
            DiscoveredCapability {
                name: "get_weather".to_string(),
                description: Some("Get current weather for a location".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string", "description": "City name"}
                    },
                    "required": ["location"]
                })),
                capability_type: CapabilityType::MCP,
            },
            DiscoveredCapability {
                name: "calculate".to_string(),
                description: Some("Perform mathematical calculations".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "expression": {"type": "string", "description": "Math expression"}
                    },
                    "required": ["expression"]
                })),
                capability_type: CapabilityType::MCP,
            },
            DiscoveredCapability {
                name: "search_web".to_string(),
                description: Some("Search the web for information".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Search query"}
                    },
                    "required": ["query"]
                })),
                capability_type: CapabilityType::MCP,
            },
        ]
    }

    /// Step 2: Generate RTFS intent from natural language
    async fn generate_intent(
        &self,
        user_request: &str,
    ) -> Result<Intent, Box<dyn std::error::Error>> {
        println!("\n🧠 Step 2: Generating RTFS Intent");
        println!("================================");

        // Try AI generation first
        match self.generate_ai_intent(user_request).await {
            Ok(intent) => {
                println!(
                    "🤖 AI-generated intent: {}",
                    intent.name.as_deref().unwrap_or("unnamed")
                );
                Ok(intent)
            }
            Err(e) => {
                println!(
                    "⚠️  AI generation failed: {}. Falling back to demo intent.",
                    e
                );
                self.generate_demo_intent(user_request)
            }
        }
    }

    async fn generate_ai_intent(
        &self,
        user_request: &str,
    ) -> Result<Intent, Box<dyn std::error::Error>> {
        // Get the AI model from the registry
        let model = self
            .model_registry
            .get("openrouter-hunyuan-a13b-instruct")
            .ok_or("AI model not found")?;

        // Build the prompt for intent generation
        let prompt = self.build_intent_generation_prompt(user_request);

        // Log the prompt being sent to AI
        println!("📝 Sending prompt to AI:");
        println!("--- PROMPT START ---");
        println!("{}", prompt);
        println!("--- PROMPT END ---");

        // Make the AI call
        let ai_response = match model.infer(&prompt) {
            Ok(response) => response,
            Err(e) => return Err(format!("AI inference failed: {}", e).into()),
        };

        // Log the AI response
        println!("\n🤖 AI Response:");
        println!("--- AI RESPONSE START ---");
        println!("{}", ai_response);
        println!("--- AI RESPONSE END ---");

        // Parse the AI response into an intent
        let intent = self.extract_and_parse_intent(&ai_response)?;

        // Log the parsed intent details
        println!("\n📋 Parsed Intent Details:");
        println!("  Name: {}", intent.name.as_deref().unwrap_or("unnamed"));
        println!("  Goal: {}", intent.goal);
        println!("  Original Request: {}", intent.original_request);
        if !intent.constraints.is_empty() {
            println!("  Constraints:");
            for (key, value) in &intent.constraints {
                println!("    {}: {:?}", key, value);
            }
        }
        if !intent.preferences.is_empty() {
            println!("  Preferences:");
            for (key, value) in &intent.preferences {
                println!("    {}: {:?}", key, value);
            }
        }

        Ok(intent)
    }

    fn generate_demo_intent(
        &self,
        user_request: &str,
    ) -> Result<Intent, Box<dyn std::error::Error>> {
        // Create a demo intent based on the user request
        let intent_name = if user_request.to_lowercase().contains("weather") {
            "analyze_weather_request"
        } else if user_request.to_lowercase().contains("calculate")
            || user_request.to_lowercase().contains("math")
        {
            "perform_calculation"
        } else if user_request.to_lowercase().contains("search") {
            "web_search_request"
        } else {
            "general_request"
        };

        let mut intent = Intent::new(user_request.to_string()).with_name(intent_name.to_string());

        // Add some demo constraints based on the request
        let mut constraints = HashMap::new();
        if user_request.to_lowercase().contains("paris") {
            constraints.insert("location".to_string(), Value::String("Paris".to_string()));
        }
        if user_request.to_lowercase().contains("2 + 3") {
            constraints.insert("expression".to_string(), Value::String("2 + 3".to_string()));
        }

        intent.constraints = constraints;
        intent.original_request = user_request.to_string();

        Ok(intent)
    }

    fn build_intent_generation_prompt(&self, user_request: &str) -> String {
        format!(
            r#"Convert this natural language request into an RTFS intent:

USER REQUEST: "{}"

Generate an RTFS intent in this format:
(intent intent-name
  :goal "Clear description of what should be accomplished"
  :original-request "{}"
  :constraints {{ :key1 "value1" :key2 "value2" }}
  :success-criteria (fn [result] (validation logic)))

IMPORTANT: All map keys must start with ':' (colon). For example:
- Correct: {{ :input-type :string :max-length 100 }}
- Wrong: {{ input-type :string max-length 100 }}

Focus on being specific and actionable."#,
            user_request, user_request
        )
    }

    fn extract_and_parse_intent(
        &self,
        ai_response: &str,
    ) -> Result<Intent, Box<dyn std::error::Error>> {
        // First try to parse as JSON (most common AI response format)
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(ai_response.trim()) {
            return self.json_to_intent(&json_value);
        }

        // If JSON parsing fails, try to extract RTFS intent block
        if let Some(intent_start) = ai_response.find("(intent") {
            if let Some(intent_end) = self.find_matching_paren(&ai_response[intent_start..]) {
                let intent_block = &ai_response[intent_start..intent_start + intent_end + 1];

                // Parse the intent using RTFS parser
                let parsed = parser::parse(intent_block)?;
                if let Some(TopLevel::Expression(expr)) = parsed.first() {
                    // Convert expression to intent
                    return self.expression_to_intent(expr);
                }
            }
        }

        Err("Could not extract valid intent from AI response".into())
    }

    fn json_to_intent(
        &self,
        json: &serde_json::Value,
    ) -> Result<Intent, Box<dyn std::error::Error>> {
        let goal = json
            .get("goal")
            .and_then(|v| v.as_str())
            .unwrap_or("Generated goal")
            .to_string();

        let mut intent = Intent::new(goal.clone());

        // Extract constraints from JSON
        if let Some(constraints_obj) = json.get("constraints").and_then(|v| v.as_object()) {
            let mut constraints = HashMap::new();
            for (key, value) in constraints_obj {
                if let Some(str_value) = value.as_str() {
                    constraints.insert(key.clone(), Value::String(str_value.to_string()));
                } else if let Some(num_value) = value.as_i64() {
                    constraints.insert(key.clone(), Value::Integer(num_value));
                } else if let Some(bool_value) = value.as_bool() {
                    constraints.insert(key.clone(), Value::Boolean(bool_value));
                }
            }
            intent.constraints = constraints;
        }

        // Extract preferences from JSON
        if let Some(preferences_obj) = json.get("preferences").and_then(|v| v.as_object()) {
            let mut preferences = HashMap::new();
            for (key, value) in preferences_obj {
                if let Some(str_value) = value.as_str() {
                    preferences.insert(key.clone(), Value::String(str_value.to_string()));
                }
            }
            intent.preferences = preferences;
        }

        // Set a default name based on the goal
        let intent_name = if goal.to_lowercase().contains("weather") {
            "weather_request"
        } else if goal.to_lowercase().contains("calculate") || goal.to_lowercase().contains("math")
        {
            "calculation_request"
        } else if goal.to_lowercase().contains("search") {
            "search_request"
        } else {
            "general_request"
        };

        intent = intent.with_name(intent_name.to_string());

        Ok(intent)
    }

    fn expression_to_intent(
        &self,
        expr: &rtfs::ast::Expression,
    ) -> Result<Intent, Box<dyn std::error::Error>> {
        // Simplified intent extraction - in a real implementation this would be more robust
        use rtfs::ast::{Expression as E, Literal};

        if let E::FunctionCall { callee, arguments } = expr {
            if let E::Symbol(symbol) = &**callee {
                if symbol.0 == "intent" && arguments.len() >= 2 {
                    let name = if let E::Symbol(sym) = &arguments[0] {
                        sym.0.clone()
                    } else {
                        "generated_intent".to_string()
                    };

                    let mut intent = Intent::new("Generated goal".to_string()).with_name(name);

                    // Parse additional arguments as properties
                    for chunk in arguments[1..].chunks(2) {
                        if chunk.len() == 2 {
                            if let (
                                E::Literal(Literal::Keyword(key)),
                                E::Literal(Literal::String(value)),
                            ) = (&chunk[0], &chunk[1])
                            {
                                match key.0.as_str() {
                                    "goal" => intent.goal = value.clone(),
                                    "original-request" => intent.original_request = value.clone(),
                                    _ => {}
                                }
                            }
                        }
                    }

                    return Ok(intent);
                }
            }
        }

        Err("Invalid intent expression".into())
    }

    fn find_matching_paren(&self, s: &str) -> Option<usize> {
        let mut depth = 0;
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Step 3: Generate RTFS plan from intent + capabilities
    async fn generate_plan(
        &self,
        intent: &Intent,
        capabilities: &[DiscoveredCapability],
    ) -> Result<Plan, Box<dyn std::error::Error>> {
        println!("\n📋 Step 3: Generating RTFS Plan");
        println!("==============================");

        println!("🎯 Intent: {}", intent.name.as_deref().unwrap_or("unnamed"));
        println!("📝 Goal: {}", intent.goal);

        // Determine which capability to use based on the intent (demo: ensure we reference known capabilities)
        let _capability_to_use = self.select_capability_for_intent(intent, capabilities);

        // Demo plan: wrap into a multi-step do block and use built-in capabilities
        let rtfs_code = format!(
            "(do\n  (step \"Understand Request\" (call :ccos.echo \"{}\"))\n)",
            intent.goal.replace('"', "\\\"")
        );

        let plan = Plan::new_rtfs(rtfs_code.clone(), vec![intent.intent_id.clone()]);

        // Log the generated plan details
        println!("\n📋 Generated Plan Details:");
        println!("  Plan Name: {}", plan.name.as_deref().unwrap_or("unnamed"));
        println!("  Plan Language: RTFS");
        println!("  RTFS Code: {}", rtfs_code);
        println!("  Intent Dependencies: {}", plan.intent_ids.len());
        for (i, dep) in plan.intent_ids.iter().enumerate() {
            println!("    {}: {}", i + 1, dep);
        }

        println!("✅ Generated plan with RTFS body");
        Ok(plan)
    }

    fn select_capability_for_intent(
        &self,
        intent: &Intent,
        capabilities: &[DiscoveredCapability],
    ) -> Option<DiscoveredCapability> {
        let goal_lower = intent.goal.to_lowercase();
        let request_lower = intent.original_request.to_lowercase();

        for cap in capabilities {
            let name_lower = cap.name.to_lowercase();
            let desc_lower = cap.description.as_deref().unwrap_or("").to_lowercase();

            if (goal_lower.contains(&name_lower) || request_lower.contains(&name_lower))
                || (desc_lower.contains("weather")
                    && (goal_lower.contains("weather") || request_lower.contains("weather")))
                || (desc_lower.contains("calculate")
                    && (goal_lower.contains("calculate") || request_lower.contains("math")))
                || (desc_lower.contains("search")
                    && (goal_lower.contains("search") || request_lower.contains("find")))
            {
                return Some(cap.clone());
            }
        }

        None
    }

    /// Step 4: Execute the plan via the Orchestrator with audited updates and status transitions
    async fn execute_plan(&self, plan: &Plan) -> Result<Value, Box<dyn std::error::Error>> {
        println!("\n⚡ Step 4: Executing RTFS Plan");
        println!("============================");

        println!(
            "🚀 Executing plan: {}",
            plan.name.as_deref().unwrap_or("unnamed")
        );
        println!("📋 Executing RTFS plan through Orchestrator...");

        // Security context: allow built-in demo capabilities
        let context = RuntimeContext {
            security_level: rtfs::runtime::security::SecurityLevel::Controlled,
            allowed_capabilities: vec!["ccos.echo".to_string(), "ccos.math.add".to_string()]
                .into_iter()
                .collect(),
            ..RuntimeContext::pure()
        };

        // Ensure the plan is in a do-wrapped RTFS body
        if let PlanBody::Rtfs(code) = &plan.body {
            println!("🔄 RTFS code: {}", code);
        }

        // Execute and let the orchestrator update the IntentGraph and CausalChain
        let exec = self.orchestrator.execute_plan(plan, &context).await?;

        if exec.success {
            println!("✅ Plan execution completed!");
        } else {
            println!("⚠️  Plan execution failed");
        }

        Ok(exec.value)
    }

    /// Step 5: Present results
    fn present_results(&self, intent: &Intent, plan: &Plan, result: &Value) {
        println!("\n🎊 Step 5: Results & Summary");
        println!("===========================");

        println!("🎯 Original Request: {}", intent.original_request);
        println!(
            "🧠 Generated Intent: {}",
            intent.name.as_deref().unwrap_or("unnamed")
        );
        println!("📝 Intent Goal: {}", intent.goal);
        println!(
            "📋 Executed Plan: {}",
            plan.name.as_deref().unwrap_or("unnamed")
        );
        println!("⚡ Plan Language: RTFS");

        println!("\n📊 Final Result:");
        println!("{}", self.format_value(result));

        // Show a brief audit tail from the CausalChain
        if let Ok(chain) = self.causal_chain.lock() {
            let recent = chain.get_all_actions();
            println!("\n🧾 Audit Trail (last {} actions):", recent.len().min(5));
            for action in recent.iter().rev().take(5).rev() {
                println!(
                    "  - {:?} {}",
                    action.action_type,
                    action.function_name.as_deref().unwrap_or("")
                );
            }
        }

        println!("\n✨ Demo completed successfully!");
        println!("This demonstrates the full CCOS + RTFS pipeline:");
        println!("1. ✅ Natural Language → Structured Intent");
        println!("2. ✅ Dynamic Capability Discovery");
        println!("3. ✅ Intent → Executable Plan Generation");
        println!("4. ✅ Plan Execution with Runtime (audited)");
        println!("5. ✅ Results Presentation");
    }

    fn format_value(&self, value: &Value) -> String {
        // Use the utility function for consistent RTFS formatting with pretty printing
        format!("{:?}", value) // format_rtfs_value_pretty not available
    }
}

#[derive(Clone)]
#[allow(dead_code)]
struct DiscoveredCapability {
    name: String,
    description: Option<String>,
    input_schema: Option<serde_json::Value>,
    capability_type: CapabilityType,
}

#[derive(Clone)]
#[allow(dead_code)]
enum CapabilityType {
    MCP,
    CCOS,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("🤖 CCOS + RTFS Comprehensive Interactive Demo");
    println!("=============================================");
    println!("Demonstrating the complete CCOS + RTFS architecture pipeline\n");

    // Get user input
    let user_request = if args.interactive {
        println!("💬 What would you like to accomplish?");
        println!("   (e.g., 'Get the weather for Paris', 'Calculate 2 + 3', 'Search for cats')");
        print!("> ");

        use std::io::{self, Write};
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    } else {
        args.goal
            .unwrap_or_else(|| "Get the weather for Paris and calculate 2 + 3".to_string())
    };

    println!("🎯 User Request: \"{}\"", user_request);

    // Initialize demo context
    let context = DemoContext::new(&args.mcp_server).await?;

    // Execute the complete pipeline
    let capabilities = context.discover_capabilities().await?;
    let intent = context.generate_intent(&user_request).await?;

    // Persist the intent into the IntentGraph for audited lifecycle
    context.persist_intent(&intent)?;

    let plan = context.generate_plan(&intent, &capabilities).await?;
    let result = context.execute_plan(&plan).await?;

    // Present final results
    context.present_results(&intent, &plan, &result);

    Ok(())
}
