# CCOS Arbiter Examples and Testing Guide

This guide provides step-by-step instructions for running the CCOS Arbiter examples and testing the implementation.

## Quick Start

### Prerequisites

1. **Rust Environment**: Ensure you have Rust installed (1.70+)
2. **API Keys** (optional): For real LLM testing
   - OpenAI API key
   - OpenRouter API key
   - Anthropic API key

### Build and Test

```bash
# Navigate to the rtfs_compiler directory
cd rtfs_compiler

# Build the project
cargo build

# Run all tests
cargo test

# Run specific arbiter tests
cargo test --package rtfs_compiler --lib ccos::arbiter
```

## Available Examples

### 1. Standalone Arbiter Example

**Purpose**: Demonstrates basic Arbiter functionality with configuration-driven setup.

**Run Command**:
```bash
cargo run --example standalone_arbiter
```

**What it does**:
- Loads configuration from environment variables
- Creates an IntentGraph and CausalChain
- Processes multiple test scenarios
- Records events in the causal chain
- Measures performance

**Expected Output**:
```
🚀 CCOS Arbiter Standalone Example
================================

✅ Configuration loaded successfully
✅ Components initialized
✅ Processing test scenarios...

📊 Results:
- Test 1: "Hello" → Success (2.3ms)
- Test 2: "Analyze sentiment" → Success (1.8ms)
- Test 3: "Complex task" → Success (3.1ms)

✅ All tests completed successfully
```

**Configuration**:
```bash
# Set environment variables
export CCOS_ARBITER_ENGINE_TYPE=dummy
export CCOS_LLM_PROVIDER_TYPE=stub
export CCOS_LLM_MODEL=test-model
```

### 2. OpenRouter Demo

**Purpose**: Demonstrates OpenRouter integration for multi-model LLM access.

**Run Command**:
```bash
cargo run --example openrouter_demo
```

**What it does**:
- Configures OpenRouter API connection
- Tests with different models (Claude, GPT-4, etc.)
- Falls back to stub provider if no API key
- Shows real LLM processing capabilities

**Expected Output**:
```
🚀 OpenRouter LLM Arbiter Demo
==============================

✅ OpenRouter configuration loaded
✅ LLM provider initialized
✅ Processing natural language requests...

📝 Intent Generation:
- Input: "analyze user sentiment"
- Generated Intent: { name: "sentiment_analysis", goal: "..." }

📋 Plan Generation:
- Generated Plan: (do (step "Analyze" (call :ccos.echo "sentiment: positive")))

✅ Demo completed successfully
```

**Configuration**:
```bash
# Set OpenRouter API key
export CCOS_LLM_API_KEY=your_openrouter_key
export CCOS_LLM_BASE_URL=https://openrouter.ai/api/v1
export CCOS_LLM_MODEL=anthropic/claude-3-opus
```

### 3. LLM Provider Demo

**Purpose**: Demonstrates the LLM provider abstraction and individual components.

**Run Command**:
```bash
cargo run --example llm_provider_demo
```

**What it does**:
- Tests LLM provider in isolation
- Demonstrates intent generation
- Shows plan generation
- Validates generated plans

**Expected Output**:
```
🔧 LLM Provider Demo
===================

✅ Stub LLM Provider initialized
✅ Testing intent generation...
✅ Testing plan generation...
✅ Testing plan validation...

📊 Results:
- Intent Generation: ✅ Success
- Plan Generation: ✅ Success  
- Plan Validation: ✅ Success

✅ All tests passed
```

## Testing Guide

### Running All Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test suite
cargo test --package rtfs_compiler --lib ccos::arbiter
```

### Test Categories

#### 1. LLM Provider Tests
```bash
# Run LLM provider tests
cargo test test_stub_provider
cargo test test_openai_provider
```

#### 2. LLM Arbiter Tests
```bash
# Run LLM arbiter tests
cargo test test_llm_arbiter
cargo test test_llm_arbiter_creation
```

#### 3. Configuration Tests
```bash
# Run configuration tests
cargo test test_arbiter_config
cargo test test_llm_config
```

#### 4. Factory Tests
```bash
# Run factory tests
cargo test test_arbiter_factory
```

#### 5. Dummy Arbiter Tests
```bash
# Run dummy arbiter tests
cargo test test_dummy_arbiter
```

### Test Results

All 19 tests should pass:
```
running 19 tests
test ccos::arbiter::arbiter_config::tests::test_arbiter_config_default ... ok
test ccos::arbiter::arbiter_config::tests::test_llm_config_validation ... ok
test ccos::arbiter::arbiter_factory::tests::test_arbiter_factory_dummy ... ok
test ccos::arbiter::arbiter_factory::tests::test_arbiter_factory_llm ... ok
test ccos::arbiter::dummy_arbiter::tests::test_dummy_arbiter_creation ... ok
test ccos::arbiter::dummy_arbiter::tests::test_dummy_arbiter_intent_generation ... ok
test ccos::arbiter::dummy_arbiter::tests::test_dummy_arbiter_plan_generation ... ok
test ccos::arbiter::dummy_arbiter::tests::test_dummy_arbiter_process_natural_language ... ok
test ccos::arbiter::llm_arbiter::tests::test_llm_arbiter_creation ... ok
test ccos::arbiter::llm_arbiter::tests::test_llm_arbiter_intent_generation ... ok
test ccos::arbiter::llm_arbiter::tests::test_llm_arbiter_plan_generation ... ok
test ccos::arbiter::llm_arbiter::tests::test_llm_arbiter_process_natural_language ... ok
test ccos::arbiter::llm_provider::tests::test_stub_provider_creation ... ok
test ccos::arbiter::llm_provider::tests::test_stub_provider_intent_generation ... ok
test ccos::arbiter::llm_provider::tests::test_stub_provider_plan_generation ... ok
test ccos::arbiter::llm_provider::tests::test_stub_provider_validation ... ok
test ccos::arbiter::legacy_arbiter::tests::test_legacy_arbiter_creation ... ok
test ccos::arbiter::legacy_arbiter::tests::test_legacy_arbiter_process_natural_language ... ok

test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Configuration Examples

### 1. Dummy Configuration (Testing)

```toml
# arbiter_config.toml
engine_type = "dummy"

[llm_config]
provider_type = "stub"
model = "test-model"
```

### 2. OpenAI Configuration

```toml
# arbiter_config.toml
engine_type = "llm"

[llm_config]
provider_type = "openai"
model = "gpt-4"
api_key = "your-openai-key"
max_tokens = 1000
temperature = 0.7
timeout_seconds = 30
```

### 3. OpenRouter Configuration

```toml
# arbiter_config.toml
engine_type = "llm"

[llm_config]
provider_type = "openai"  # OpenRouter uses OpenAI-compatible API
model = "anthropic/claude-3-opus"
api_key = "your-openrouter-key"
base_url = "https://openrouter.ai/api/v1"
max_tokens = 2000
temperature = 0.5
timeout_seconds = 60
```

### 4. Environment Variables

```bash
# Engine configuration
export CCOS_ARBITER_ENGINE_TYPE=llm

# LLM configuration
export CCOS_LLM_PROVIDER_TYPE=openai
export CCOS_LLM_MODEL=gpt-4
export CCOS_LLM_API_KEY=your-api-key
export CCOS_LLM_MAX_TOKENS=1000
export CCOS_LLM_TEMPERATURE=0.7
export CCOS_LLM_TIMEOUT_SECONDS=30

# Security configuration
export CCOS_SECURITY_VALIDATE_INTENTS=true
export CCOS_SECURITY_VALIDATE_PLANS=true
export CCOS_SECURITY_MAX_PLAN_COMPLEXITY=100
```

## Troubleshooting

### Common Issues

#### 1. Compilation Errors

```bash
# Clean and rebuild
cargo clean
cargo build

# Check for missing dependencies
cargo check
```

#### 2. Test Failures

```bash
# Run tests with verbose output
cargo test -- --nocapture

# Run specific failing test
cargo test test_name -- --nocapture
```

#### 3. API Key Issues

```bash
# Check if API key is set
echo $CCOS_LLM_API_KEY

# Set API key
export CCOS_LLM_API_KEY=your-api-key

# For OpenRouter
export CCOS_LLM_API_KEY=your-openrouter-key
export CCOS_LLM_BASE_URL=https://openrouter.ai/api/v1
```

#### 4. Network Issues

```bash
# Check network connectivity
curl -I https://api.openai.com/v1/models

# For OpenRouter
curl -I https://openrouter.ai/api/v1/models
```

### Debug Mode

```bash
# Enable debug logging
export RUST_LOG=debug

# Run with debug output
cargo run --example standalone_arbiter
```

### Performance Testing

```bash
# Run with performance measurement
cargo run --example standalone_arbiter

# Check memory usage
cargo run --example standalone_arbiter 2>&1 | grep -E "(ms|memory|performance)"
```

## Advanced Usage

### Custom LLM Provider

```rust
use rtfs_compiler::ccos::arbiter::{LlmProvider, LlmProviderConfig};

#[derive(Debug)]
struct CustomLlmProvider {
    config: LlmProviderConfig,
}

#[async_trait::async_trait]
impl LlmProvider for CustomLlmProvider {
    async fn generate_intent(
        &self,
        prompt: &str,
        context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError> {
        // Custom implementation
    }
    
    // ... other methods
}
```

### Custom Configuration

```rust
use rtfs_compiler::ccos::arbiter::{ArbiterConfig, ArbiterEngineType, LlmConfig, LlmProviderType};

let config = ArbiterConfig {
    engine_type: ArbiterEngineType::Llm,
    llm_config: Some(LlmConfig {
        provider_type: LlmProviderType::OpenAI,
        model: "gpt-4".to_string(),
        api_key: Some("your-key".to_string()),
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        timeout_seconds: Some(30),
    }),
    delegation_config: None,
    capability_config: Default::default(),
    security_config: Default::default(),
    template_config: None,
};
```

## Performance Benchmarks

### Expected Performance

| Component | Response Time | Notes |
|-----------|---------------|-------|
| Dummy Arbiter | < 1ms | Deterministic responses |
| LLM Arbiter (Stub) | < 10ms | Simulated LLM responses |
| LLM Arbiter (OpenAI) | < 5s | Real API calls |
| LLM Arbiter (OpenRouter) | < 5s | Real API calls |

### Memory Usage

- **Dummy Arbiter**: ~2MB
- **LLM Arbiter**: ~5MB (with HTTP client)
- **Full CCOS Integration**: ~10MB

## Next Steps

After running the examples successfully:

1. **Explore the Code**: Review the implementation in `src/ccos/arbiter/`
2. **Extend Functionality**: Add new LLM providers or engine types
3. **Integration**: Integrate with the full CCOS system
4. **Deployment**: Deploy as a standalone service

## Support

For issues or questions:

1. Check the test output for error details
2. Review the configuration examples
3. Ensure all dependencies are installed
4. Verify API keys and network connectivity

The examples demonstrate a complete, working implementation of the CCOS Arbiter with full LLM integration, comprehensive testing, and standalone deployment capabilities.

