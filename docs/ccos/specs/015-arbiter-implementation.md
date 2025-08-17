# CCOS Arbiter Implementation Specification

## Overview

This specification defines the implementation of the CCOS Arbiter component, which serves as the cognitive layer responsible for converting natural language requests into structured intents and executable RTFS plans.

## Architecture

### Core Components

1. **ArbiterEngine Trait**: Abstract interface for all arbiter implementations
2. **Configuration System**: TOML-based configuration for different arbiter types
3. **Factory Pattern**: Dynamic creation of arbiter instances based on configuration
4. **Multiple Engine Types**: Template, LLM, Delegating, Hybrid, and Dummy implementations

### Engine Types

#### Dummy Arbiter
- **Purpose**: Deterministic testing and CI/CD pipelines
- **Features**: Pattern-based intent generation, predefined RTFS plans
- **Use Cases**: Unit tests, integration tests, development environments

#### LLM Arbiter
- **Purpose**: AI-driven intent and plan generation
- **Features**: Natural language understanding, dynamic plan creation
- **Use Cases**: Production environments, complex reasoning tasks

#### Delegating Arbiter
- **Purpose**: Multi-agent collaboration and delegation
- **Features**: Agent discovery, capability matching, distributed execution
- **Use Cases**: Complex workflows, specialized domain expertise

#### Template Arbiter
- **Purpose**: Rule-based intent and plan generation
- **Features**: Pattern matching, predefined templates, fast execution
- **Use Cases**: Simple workflows, predictable responses

#### Hybrid Arbiter
- **Purpose**: Combination of multiple approaches
- **Features**: Fallback mechanisms, adaptive strategy selection
- **Use Cases**: Robust production systems, mixed complexity workloads

## Configuration

### TOML Configuration Structure

```toml
# Engine type: template, llm, delegating, hybrid, dummy
engine_type = "dummy"

# LLM configuration (required for llm, delegating, and hybrid engines)
[llm_config]
provider = "stub"  # openai, anthropic, stub, echo
model = "gpt-4"
api_key = ""  # Set via environment variable CCOS_LLM_API_KEY
max_tokens = 4096
temperature = 0.7
system_prompt = "You are a helpful AI assistant that generates structured intents and RTFS plans."
intent_prompt_template = "Convert the following natural language request into a structured Intent: {input}"
plan_prompt_template = "Generate an RTFS plan to achieve this intent: {intent}"

# Delegation configuration (required for delegating engine)
[delegation_config]
enabled = true
max_delegation_depth = 3
agent_discovery_timeout = 30
capability_matching_threshold = 0.8

# Capability configuration
[capability_config]
allowed_prefixes = ["ccos.", "std.", "math.", "string."]
blocked_prefixes = ["system.", "admin.", "root."]
max_complexity = 100

# Security configuration
[security_config]
max_plan_complexity = 50
blocked_capability_prefixes = ["system.", "admin.", "root."]
allowed_capability_prefixes = ["ccos.", "std.", "math.", "string."]
require_attestation = true
max_execution_time = 300

# Template configuration (required for template engine)
[template_config]
patterns = [
    { pattern = "analyze.*data", intent = "data_analysis", plan = "data_analysis_plan" },
    { pattern = "optimize.*performance", intent = "performance_optimization", plan = "optimization_plan" }
]
variables = []
```

## Implementation Details

### ArbiterEngine Trait

```rust
#[async_trait(?Send)]
pub trait ArbiterEngine {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError>;

    async fn intent_to_plan(
        &self,
        intent: &Intent,
    ) -> Result<Plan, RuntimeError>;

    async fn execute_plan(
        &self,
        plan: &Plan,
    ) -> Result<ExecutionResult, RuntimeError>;

    async fn learn_from_execution(
        &self,
        intent: &Intent,
        plan: &Plan,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError>;
}
```

### Dummy Arbiter Implementation

The dummy arbiter provides deterministic responses for testing:

```rust
pub struct DummyArbiter {
    config: ArbiterConfig,
    intent_graph: Arc<Mutex<IntentGraph>>,
}

impl DummyArbiter {
    pub fn new(config: ArbiterConfig, intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self { config, intent_graph }
    }

    fn generate_dummy_intent(&self, nl: &str) -> Intent {
        // Pattern-based intent generation
        let lower_nl = nl.to_lowercase();
        
        if lower_nl.contains("sentiment") || lower_nl.contains("analyze") {
            // Generate sentiment analysis intent
        } else if lower_nl.contains("optimize") || lower_nl.contains("improve") {
            // Generate optimization intent
        } else {
            // Generate general assistance intent
        }
    }

    fn generate_dummy_plan(&self, intent: &Intent) -> Plan {
        // Generate RTFS plan based on intent type
        match intent.name.as_deref() {
            Some("analyze_user_sentiment") => {
                // Generate sentiment analysis plan
            }
            Some("optimize_response_time") => {
                // Generate optimization plan
            }
            _ => {
                // Generate default plan
            }
        }
    }
}
```

### Factory Pattern

```rust
pub struct ArbiterFactory;

impl ArbiterFactory {
    pub async fn create_arbiter(
        config: ArbiterConfig,
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Result<Box<dyn ArbiterEngine>, RuntimeError> {
        match config.engine_type {
            ArbiterEngineType::Dummy => {
                Ok(Box::new(DummyArbiter::new(config, intent_graph)))
            }
            ArbiterEngineType::Llm => {
                // Create LLM arbiter
                todo!("Implement LLM arbiter")
            }
            ArbiterEngineType::Delegating => {
                // Create delegating arbiter
                todo!("Implement delegating arbiter")
            }
            ArbiterEngineType::Template => {
                // Create template arbiter
                todo!("Implement template arbiter")
            }
            ArbiterEngineType::Hybrid => {
                // Create hybrid arbiter
                todo!("Implement hybrid arbiter")
            }
        }
    }
}
```

## Security Considerations

### Plan Validation

All generated plans must be validated against security constraints:

1. **Complexity Limits**: Maximum number of steps and nested expressions
2. **Capability Restrictions**: Allowed and blocked capability prefixes
3. **Execution Time Limits**: Maximum allowed execution time
4. **Resource Constraints**: Memory and CPU usage limits

### Attestation

For production environments, all capability calls must be attested:

1. **Digital Signatures**: Verify capability provider authenticity
2. **Provenance Tracking**: Complete chain of custody
3. **Content Hashing**: SHA-256 integrity verification

## Testing Strategy

### Unit Tests

- **Dummy Arbiter**: Test pattern matching and plan generation
- **Configuration**: Test TOML parsing and validation
- **Factory**: Test arbiter creation and error handling

### Integration Tests

- **End-to-End**: Complete intent-to-execution workflow
- **Security**: Plan validation and capability restrictions
- **Performance**: Execution time and resource usage

### Example Test

```rust
#[tokio::test]
async fn test_dummy_arbiter_sentiment_analysis() {
    let config = ArbiterConfig::default();
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(IntentGraphConfig::default()).await.unwrap()));
    let arbiter = DummyArbiter::new(config, intent_graph);

    let intent = arbiter.natural_language_to_intent(
        "Analyze the sentiment of user feedback",
        None
    ).await.unwrap();

    assert_eq!(intent.name, Some("analyze_user_sentiment".to_string()));
    assert!(intent.goal.contains("sentiment"));

    let plan = arbiter.intent_to_plan(&intent).await.unwrap();
    assert!(matches!(plan.body, PlanBody::Rtfs(_)));
}
```

## Deployment

### Standalone Mode

The arbiter can be deployed as a standalone service:

```bash
# Run with configuration file
cargo run --example standalone_arbiter -- --config arbiter_config.toml

# Run with environment variables
CCOS_LLM_API_KEY=your_key cargo run --example standalone_arbiter
```

### Library Mode

The arbiter can be integrated into larger CCOS systems:

```rust
use rtfs_compiler::ccos::{
    arbiter_config::ArbiterConfig,
    arbiter_factory::ArbiterFactory,
    intent_graph::IntentGraph,
};

let config = ArbiterConfig::from_file("arbiter_config.toml")?;
let intent_graph = Arc::new(Mutex::new(IntentGraph::new_async(config.clone()).await?));
let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await?;

let intent = arbiter.natural_language_to_intent("Help me analyze data", None).await?;
let plan = arbiter.intent_to_plan(&intent).await?;
let result = arbiter.execute_plan(&plan).await?;
```

## Future Enhancements

1. **Learning Capabilities**: Improve intent and plan generation based on execution results
2. **Multi-Modal Support**: Handle images, audio, and other input types
3. **Federated Learning**: Share insights across multiple arbiter instances
4. **Advanced Security**: Zero-knowledge proofs and homomorphic encryption
5. **Performance Optimization**: Caching, parallel execution, and resource management

## References

- [CCOS Architecture Specification](000-ccos-architecture.md)
- [Intent Graph Specification](001-intent-graph.md)
- [Plans and Orchestration Specification](002-plans-and-orchestration.md)
- [RTFS 2.0 Language Specification](../../rtfs-2.0/specs/10-formal-language-specification.md)
