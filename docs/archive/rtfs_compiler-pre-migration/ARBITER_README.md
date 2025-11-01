# CCOS Arbiter Implementation

A modular, configurable Arbiter implementation for CCOS (Cognitive Computing Operating System) that converts natural language requests into structured intents and executable RTFS plans.

## Features

### ✅ Completed Features
1. **LLM Provider System**:
   - ✅ `LlmProvider` trait with async support
   - ✅ `StubLlmProvider` for testing
   - ✅ `OpenAIProvider` for OpenAI API
   - ✅ `AnthropicLlmProvider` for Anthropic Claude API
   - ✅ OpenRouter support (OpenAI-compatible)
   - ✅ Configuration-driven provider selection

2. **Arbiter Engines**:
   - ✅ `DummyArbiter` - Deterministic testing implementation
   - ✅ `LlmArbiter` - LLM-driven intent and plan generation
   - ✅ `TemplateArbiter` - Pattern matching and templates
   - ✅ `HybridArbiter` - Template + LLM fallback
   - ✅ `DelegatingArbiter` - LLM + agent delegation (structure complete, parsing issue)
   - ✅ Factory pattern for engine creation

3. **Configuration System**:
   - ✅ TOML-based configuration
   - ✅ Environment variable support
   - ✅ Validation and error handling
   - ✅ Default configurations for all components

4. **Testing & Examples**:
   - ✅ Comprehensive test suite
   - ✅ Standalone demo applications
   - ✅ OpenRouter integration demo
   - ✅ Anthropic Claude integration demo
   - ✅ All arbiter engines demo (Template, Hybrid, LLM working, Delegating has parsing issue)

## Quick Start

### 1. Basic Usage

```rust
use rtfs_compiler::ccos::arbiter::{
    ArbiterConfig, LlmConfig, LlmProviderType, ArbiterFactory,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = ArbiterConfig {
        engine_type: rtfs_compiler::ccos::arbiter::ArbiterEngineType::Llm,
        llm_config: Some(LlmConfig {
            provider_type: LlmProviderType::Anthropic, // or OpenAI, Stub
            model: "claude-3-sonnet-20240229".to_string(),
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            base_url: None,
            max_tokens: Some(2000),
            temperature: Some(0.7),
            timeout_seconds: Some(60),
        }),
        ..Default::default()
    };

    // Create intent graph
    let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::intent_graph::IntentGraph::new()?
    ));

    // Create arbiter
    let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await?;

    // Process natural language request
    let result = arbiter.process_natural_language(
        "analyze user sentiment from recent interactions",
        None
    ).await?;

    println!("Result: {}", result.value);
    Ok(())
}
```

### 2. Configuration File

Create `arbiter_config.toml`:

```toml
# Engine type: dummy, llm, template, hybrid, delegating
engine_type = "llm"

# LLM configuration
[llm_config]
provider_type = "anthropic"  # openai, anthropic, stub
model = "claude-3-sonnet-20240229"
api_key = "your-api-key"  # or set ANTHROPIC_API_KEY env var
max_tokens = 2000
temperature = 0.7
timeout_seconds = 60

# Security configuration
[security_config]
allowed_capability_prefixes = ["ccos.", "system."]
max_plan_complexity = 100
require_attestation = true

# Capability configuration
[capability_config]
enable_marketplace = true
cache_capabilities = true
```

### 3. Environment Variables

```bash
# For Anthropic Claude
export ANTHROPIC_API_KEY="your-anthropic-api-key"

# For OpenAI/OpenRouter
export OPENAI_API_KEY="your-openai-api-key"
export OPENROUTER_API_KEY="your-openrouter-api-key"
```

## Examples

### Run Demos

```bash
# Anthropic Claude demo
cargo run --example anthropic_demo

# OpenRouter demo (OpenAI-compatible)
cargo run --example openrouter_demo

# LLM Provider demo
cargo run --example llm_provider_demo

# Standalone arbiter demo
cargo run --example standalone_arbiter
```

### Available Examples

1. **`anthropic_demo`** - Demonstrates Anthropic Claude integration
2. **`openrouter_demo`** - Shows OpenRouter (OpenAI-compatible) usage
3. **`llm_provider_demo`** - Tests LLM provider abstraction
4. **`standalone_arbiter`** - Complete standalone CCOS Arbiter instance

## Architecture

### LLM Providers

The system supports multiple LLM providers through a unified interface:

- **`StubLlmProvider`** - Deterministic responses for testing
- **`OpenAILlmProvider`** - OpenAI GPT models and OpenRouter
- **`AnthropicLlmProvider`** - Anthropic Claude models

### Arbiter Engines

Different engines for various use cases:

- **`DummyArbiter`** - Hard-coded responses for testing
- **`LlmArbiter`** - LLM-driven intent and plan generation
- **`TemplateArbiter`** - Pattern-based matching (planned)
- **`HybridArbiter`** - Template + LLM fallback (planned)
- **`DelegatingArbiter`** - Agent delegation (planned)

### Configuration System

Flexible configuration with:
- TOML file support
- Environment variable integration
- Validation and error handling
- Default configurations

## API Reference

### Core Types

```rust
pub struct ArbiterConfig {
    pub engine_type: ArbiterEngineType,
    pub llm_config: Option<LlmConfig>,
    pub security_config: SecurityConfig,
    pub capability_config: CapabilityConfig,
    // ...
}

pub struct LlmConfig {
    pub provider_type: LlmProviderType,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub timeout_seconds: Option<u64>,
}

pub enum LlmProviderType {
    Stub,
    OpenAI,
    Anthropic,
    Local,
}
```

### Main Interface

```rust
#[async_trait]
pub trait ArbiterEngine {
    async fn process_natural_language(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<ExecutionResult, RuntimeError>;
}
```

## Testing

Run the test suite:

```bash
# All tests
cargo test

# Specific test categories
cargo test test_anthropic_provider
cargo test test_stub_provider
cargo test test_openai_provider
```

## Configuration Examples

### Anthropic Claude

```toml
[llm_config]
provider_type = "anthropic"
model = "claude-3-sonnet-20240229"
api_key = "your-anthropic-api-key"
max_tokens = 2000
temperature = 0.7
timeout_seconds = 60
```

### OpenAI/OpenRouter

```toml
[llm_config]
provider_type = "openai"
model = "gpt-4"
api_key = "your-openai-api-key"
base_url = "https://openrouter.ai/api/v1"  # Optional for OpenRouter
max_tokens = 2000
temperature = 0.7
timeout_seconds = 60
```

### Stub Provider (Testing)

```toml
[llm_config]
provider_type = "stub"
model = "stub-model"
# No API key needed
max_tokens = 1000
temperature = 0.7
timeout_seconds = 30
```

## Development

### Adding New LLM Providers

1. Implement the `LlmProvider` trait
2. Add provider type to `LlmProviderType` enum
3. Update factory in `LlmProviderFactory`
4. Add tests and examples

### Adding New Arbiter Engines

1. Implement the `ArbiterEngine` trait
2. Add engine type to `ArbiterEngineType` enum
3. Update factory in `ArbiterFactory`
4. Add tests and examples

## Status

### ✅ Completed
- Core Arbiter refactoring
- LLM integration (OpenAI, Anthropic, Stub)
- Configuration system
- Testing framework
- Standalone deployment
- OpenRouter support
- Anthropic Claude support

### ✅ Completed
- Delegating Arbiter parsing issue fixed (delegation analysis JSON parsing working)
- Prompt management system integration (centralized, versioned prompts)

### 🚧 In Progress
- Enhanced prompt templates with RTFS 2.0 grammar
- Remote prompt stores (git/http)
- Provenance logging in Causal Chain

### 📋 Planned
- Performance monitoring and metrics
- Advanced testing (property-based, fuzzing)
- Enhanced prompt templates with RTFS 2.0 grammar
- Capability marketplace integration
- Remote prompt stores (git/http)
- Provenance logging in Causal Chain

## Contributing

1. Follow the existing code structure
2. Add tests for new features
3. Update documentation
4. Ensure all tests pass

## License

This project is part of the CCOS (Cognitive Computing Operating System) implementation.
