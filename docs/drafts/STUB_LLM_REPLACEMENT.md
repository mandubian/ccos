# Stub LLM Provider Replacement

**Status**: ✅ Completed (2025-01-03)  
**Goal**: Replace StubLlmProvider with real LLM providers in production code

## Changes Made

### 1. Factory-Level Protection ✅

**File**: `ccos/src/arbiter/llm_provider.rs`

The `LlmProviderFactory::create_provider()` now:
- **Rejects Stub provider** unless explicitly enabled via `CCOS_ALLOW_STUB_PROVIDER=1`
- **Allows Stub in test mode** (via `cfg!(test)`)
- **Shows warnings** when Stub is used
- **Provides helpful error messages** suggesting real providers

```rust
LlmProviderType::Stub => {
    let allow_stub = std::env::var("CCOS_ALLOW_STUB_PROVIDER")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
        || cfg!(test);
    
    if !allow_stub {
        return Err(RuntimeError::Generic(
            "Stub LLM provider is not allowed in production. Set CCOS_ALLOW_STUB_PROVIDER=1 to enable (for testing only), or use a real provider (openai, anthropic, openrouter).".to_string()
        ));
    }
    
    eprintln!("⚠️  WARNING: Using Stub LLM Provider (testing only - not realistic)");
    Ok(Box::new(StubLlmProvider::new(config)))
}
```

### 2. OpenRouter Support ✅

**File**: `ccos/src/arbiter/arbiter_config.rs`

- Maps `"openrouter"` → `LlmProviderType::OpenAI` (uses OpenAI-compatible API)
- Automatically sets base URL to `https://openrouter.ai/api/v1` if not set
- OpenRouter uses the same `OpenAILlmProvider` with different base URL

### 3. Configuration Warnings ✅

**File**: `ccos/examples/smart_assistant_demo.rs`

- Shows warnings when stub profile is selected
- Sets `CCOS_ALLOW_STUB_PROVIDER=1` when stub is explicitly requested
- Provides guidance on using real providers

### 4. Default Configuration ✅

**File**: `config/agent_config.toml`

- Default profile: `"openrouter_free:balanced"` (real provider)
- Stub profile marked as "ONLY for testing"
- Clear documentation in comments

## Usage

### Production (Real LLM)

```bash
# Default (uses openrouter_free:balanced from agent_config.toml)
cargo run --example smart_assistant_demo -- --config config/agent_config.toml

# Explicitly use OpenRouter
export OPENROUTER_API_KEY="your_key"
cargo run --example smart_assistant_demo -- --config config/agent_config.toml --profile openrouter_free:balanced

# Use OpenAI
export OPENAI_API_KEY="your_key"
export CCOS_LLM_PROVIDER="openai"
export CCOS_LLM_MODEL="gpt-4"
cargo run --example smart_assistant_demo -- --config config/agent_config.toml
```

### Testing (Stub - Only When Needed)

```bash
# Explicitly enable stub for testing
export CCOS_ALLOW_STUB_PROVIDER=1
cargo run --example smart_assistant_demo -- --config config/agent_config.toml --profile stub/dev

# Or in test code
#[cfg(test)]
mod tests {
    // Stub provider is automatically allowed in test mode
}
```

## Error Messages

If someone tries to use Stub without enabling it:

```
Error: Stub LLM provider is not allowed in production. Set CCOS_ALLOW_STUB_PROVIDER=1 to enable (for testing only), or use a real provider (openai, anthropic, openrouter).
```

## Benefits

1. **Prevents accidental stub usage** - Production code must use real LLM
2. **Clear warnings** - Users know when stub is being used
3. **Test-friendly** - Stub still works in tests automatically
4. **OpenRouter support** - Works seamlessly with OpenRouter API
5. **Better defaults** - Default configuration uses real providers

## Migration Notes

If you have existing code using Stub:
- **Option 1**: Set `CCOS_ALLOW_STUB_PROVIDER=1` (for testing only)
- **Option 2**: Switch to real provider (recommended)
- **Option 3**: Use test mode (`#[cfg(test)]`) - Stub allowed automatically

## Next Steps

- ✅ Factory protection in place
- ✅ OpenRouter support working
- ✅ Warnings added
- ✅ Defaults updated
- ⏳ **Remaining**: Ensure all examples use real providers by default






















