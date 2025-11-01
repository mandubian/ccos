# Remote Model Providers Guide

This guide explains how to use remote LLM providers (OpenAI, Gemini, Claude, OpenRouter) with the RTFS delegation engine.

## Overview

The RTFS compiler now supports multiple remote LLM providers through a unified interface. All providers implement the `ModelProvider` trait, making them seamlessly compatible with the delegation engine.

## Supported Providers

### 1. OpenAI
- **Models**: GPT-4, GPT-3.5-turbo, GPT-4-turbo
- **API**: REST API with streaming support
- **Features**: Function calling, structured output

### 2. Google Gemini
- **Models**: Gemini Pro, Gemini Pro Vision
- **API**: Google AI Studio API
- **Features**: Multimodal support, safety filters

### 3. Anthropic Claude
- **Models**: Claude-3 Opus, Claude-3 Sonnet, Claude-3 Haiku
- **API**: Anthropic API
- **Features**: Constitutional AI, long context

### 4. OpenRouter
- **Models**: Access to 100+ models from various providers
- **API**: Unified API for multiple providers
- **Features**: Model routing, cost optimization

## Configuration

### Environment Variables

Set your API keys as environment variables:

```bash
# OpenAI
export OPENAI_API_KEY=sk-your-openai-key

# Google Gemini
export GEMINI_API_KEY=your-gemini-key

# Anthropic Claude
export ANTHROPIC_API_KEY=sk-ant-your-claude-key

# OpenRouter
export OPENROUTER_API_KEY=sk-or-your-openrouter-key
```

### Model Configuration

Each provider can be configured with specific parameters:

```rust
let config = RemoteModelConfig {
    api_key: "your-api-key".to_string(),
    base_url: None, // Use default API endpoints
    model_name: "gpt-4".to_string(),
    max_tokens: Some(1000),
    temperature: Some(0.7),
    timeout_seconds: Some(30),
};
```

## Usage Examples

### Basic Usage

```rust
use rtfs_compiler::ccos::remote_models::{
    RemoteModelFactory, OpenAIModel, GeminiModel, ClaudeModel, OpenRouterModel
};

// Create providers
let openai = RemoteModelFactory::create_openai("gpt-4", None)?;
let gemini = RemoteModelFactory::create_gemini("gemini-pro", None)?;
let claude = RemoteModelFactory::create_claude("claude-3-sonnet-20240229", None)?;
let openrouter = RemoteModelFactory::create_openrouter("openai/gpt-4", None)?;

// Use with delegation engine
let mut registry = ModelRegistry::new();
registry.register("openai", Box::new(openai));
registry.register("gemini", Box::new(gemini));
registry.register("claude", Box::new(claude));
registry.register("openrouter", Box::new(openrouter));

let delegation_engine = StaticDelegationEngine::new(registry);
```

### RTFS Script Integration

```rtfs
;; Define a function that uses remote models
(defn analyze-text [text model-provider]
  (delegate-to model-provider
    (str "Analyze the following text and provide insights:\n" text)))

;; Use different providers
(analyze-text "Hello world" "openai")
(analyze-text "Hello world" "gemini")
(analyze-text "Hello world" "claude")
```

### Advanced Configuration

```rust
// Custom configuration for specific use cases
let creative_config = RemoteModelConfig {
    api_key: env::var("OPENAI_API_KEY")?,
    model_name: "gpt-4".to_string(),
    temperature: Some(0.9), // More creative
    max_tokens: Some(2000),
    timeout_seconds: Some(60),
};

let precise_config = RemoteModelConfig {
    api_key: env::var("ANTHROPIC_API_KEY")?,
    model_name: "claude-3-opus-20240229".to_string(),
    temperature: Some(0.1), // More precise
    max_tokens: Some(4000),
    timeout_seconds: Some(120),
};
```

## Error Handling

The remote model providers include comprehensive error handling:

```rust
match provider.infer_async("Your prompt").await {
    Ok(response) => println!("Response: {}", response),
    Err(e) => match e.downcast_ref::<RemoteModelError>() {
        Some(RemoteModelError::ApiKeyMissing) => {
            println!("API key not configured");
        }
        Some(RemoteModelError::RateLimited) => {
            println!("Rate limit exceeded, retry later");
        }
        Some(RemoteModelError::ModelNotFound(model)) => {
            println!("Model {} not found", model);
        }
        _ => println!("Other error: {}", e),
    }
}
```

## Cost Optimization

### OpenRouter Integration

OpenRouter provides access to multiple models with cost optimization:

```rust
// Use cheaper models for simple tasks
let cheap_model = RemoteModelFactory::create_openrouter("meta-llama/llama-2-7b-chat", None)?;

// Use expensive models for complex tasks
let expensive_model = RemoteModelFactory::create_openrouter("openai/gpt-4", None)?;
```

### Model Selection Strategy

```rust
fn select_model(task_complexity: f32) -> &'static str {
    match task_complexity {
        0.0..=0.3 => "meta-llama/llama-2-7b-chat", // Simple tasks
        0.3..=0.7 => "anthropic/claude-3-haiku-20240307", // Medium tasks
        _ => "openai/gpt-4", // Complex tasks
    }
}
```

## Security Considerations

### API Key Management

1. **Never hardcode API keys** in your source code
2. Use environment variables or secure key management systems
3. Rotate API keys regularly
4. Monitor API usage for unexpected charges

### Rate Limiting

All providers implement rate limiting:
- Respect API rate limits
- Implement exponential backoff for retries
- Use appropriate timeouts

## Testing

### Mock Providers

For testing without API keys:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_remote_model_integration() {
        // Use mock responses for testing
        let mock_provider = MockRemoteModel::new();
        // Test delegation logic
    }
}
```

### Integration Tests

```bash
# Run with API keys
OPENAI_API_KEY=sk-test cargo test remote_model_integration

# Run without API keys (will skip actual API calls)
cargo test remote_model_integration -- --ignored
```

## Performance Optimization

### Connection Pooling

The HTTP client uses connection pooling for better performance:

```rust
let client = reqwest::Client::builder()
    .pool_max_idle_per_host(10)
    .timeout(Duration::from_secs(30))
    .build()?;
```

### Caching

Integrate with the L2 inference cache:

```rust
use rtfs_compiler::ccos::caching::l2_inference::L2InferenceCache;

let cache = L2InferenceCache::new();
let cached_response = cache.get(&prompt_hash);
```

## Troubleshooting

### Common Issues

1. **API Key Not Found**
   - Check environment variable names
   - Ensure keys are properly exported

2. **Model Not Found**
   - Verify model names are correct
   - Check provider-specific model availability

3. **Rate Limiting**
   - Implement exponential backoff
   - Use multiple API keys if available

4. **Timeout Errors**
   - Increase timeout values for complex prompts
   - Check network connectivity

### Debug Mode

Enable debug logging:

```rust
use log::{debug, info};

debug!("Sending request to {}", provider.id());
info!("Response received: {}", response);
```

## Migration from Local Models

If you're migrating from local models to remote models:

```rust
// Before: Local model
let local_model = LocalLlamaModel::new("path/to/model.gguf");

// After: Remote model
let remote_model = RemoteModelFactory::create_openai("gpt-4", None)?;

// Same interface for both
let response = model.infer_async("Your prompt").await?;
```

## Best Practices

1. **Model Selection**: Choose models based on task requirements
2. **Error Handling**: Always handle API errors gracefully
3. **Cost Management**: Monitor usage and optimize for cost
4. **Security**: Secure API key management
5. **Testing**: Use mocks for unit tests
6. **Monitoring**: Log API calls and responses for debugging

## Example Projects

See the following examples for complete implementations:

- `examples/remote_model_example.rs` - Basic usage
- `examples/realistic_model_example.rs` - Local model comparison
- `examples/ccos_arbiter_demo.rtfs` - RTFS script integration 