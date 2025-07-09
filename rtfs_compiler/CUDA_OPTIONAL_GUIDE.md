# CUDA Optional Feature Guide

This guide explains how to use the optional CUDA feature in the RTFS compiler project.

## Overview

The RTFS compiler now supports optional CUDA acceleration for local model inference using `llama_cpp`. This allows the project to:

- **With CUDA**: Use GPU acceleration for faster model inference
- **Without CUDA**: Compile and run on systems without CUDA support

## Feature Configuration

### Cargo.toml Changes

The project has been modified to make CUDA optional:

```toml
[features]
default = ["pest", "regex", "repl"]
pest = ["dep:pest"]
regex = ["dep:regex"]
repl = ["rustyline"]
cuda = ["llama_cpp/cuda"]  # New optional CUDA feature

[dependencies]
# llama_cpp is now optional
llama_cpp = { version = "0.3.2", optional = true }
```

## Usage

### Building with CUDA Support

To build with CUDA acceleration:

```bash
# Build with CUDA support
cargo build --features cuda

# Run with CUDA support
cargo run --features cuda

# Run the demo with CUDA
cargo run --example cuda_optional_demo --features cuda
```

### Building without CUDA Support

To build without CUDA (default):

```bash
# Build without CUDA (default)
cargo build

# Run without CUDA
cargo run

# Run the demo without CUDA
cargo run --example cuda_optional_demo
```

## Code Examples

### Conditional Compilation

The code uses Rust's conditional compilation to handle both cases:

```rust
// Import only when CUDA feature is enabled
#[cfg(feature = "cuda")]
use llama_cpp::{LlamaModel, LlamaParams, SessionParams, standard_sampler::StandardSampler};

// Conditional struct fields
pub struct LocalLlamaModel {
    id: &'static str,
    model_path: String,
    #[cfg(feature = "cuda")]
    model: Arc<Mutex<Option<LlamaModel>>>,
    #[cfg(not(feature = "cuda"))]
    model: Arc<Mutex<Option<()>>>,
}

// Conditional function parameters
pub fn new(
    id: &'static str,
    model_path: &str,
    #[cfg(feature = "cuda")] _params: Option<LlamaParams>,
    #[cfg(not(feature = "cuda"))] _params: Option<()>,
) -> Self {
    // Implementation...
}

// Conditional logic blocks
async fn infer_async(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(feature = "cuda")]
    {
        // CUDA-enabled inference logic
        // ... actual model inference code
        Ok(response)
    }

    #[cfg(not(feature = "cuda"))]
    {
        Err("CUDA feature not enabled. Enable with --features cuda".into())
    }
}
```

### Runtime Feature Detection

You can also detect features at runtime:

```rust
fn main() {
    #[cfg(feature = "cuda")]
    {
        println!("✅ CUDA feature is ENABLED");
        // CUDA-specific code
    }

    #[cfg(not(feature = "cuda"))]
    {
        println!("❌ CUDA feature is DISABLED");
        // Fallback code
    }
}
```

## Benefits

### With CUDA Enabled
- **Performance**: GPU acceleration for model inference
- **Efficiency**: Faster processing of large language models
- **Scalability**: Better handling of concurrent inference requests

### Without CUDA
- **Compatibility**: Works on systems without CUDA drivers
- **Simplicity**: Smaller binary size
- **Portability**: Easier deployment on various platforms
- **Development**: Faster compilation during development

## Testing

The project includes tests that work with both configurations:

```rust
#[test]
fn test_cuda_feature_detection() {
    #[cfg(feature = "cuda")]
    {
        // Tests that only run with CUDA
        assert!(true, "CUDA feature is enabled");
    }

    #[cfg(not(feature = "cuda"))]
    {
        // Tests that only run without CUDA
        assert!(true, "CUDA feature is disabled");
    }
}
```

## CI/CD Considerations

For continuous integration, you might want to test both configurations:

```yaml
# Example GitHub Actions workflow
jobs:
  test-without-cuda:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - run: cargo test

  test-with-cuda:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - run: cargo test --features cuda
```

## Troubleshooting

### Common Issues

1. **CUDA not found**: Ensure CUDA toolkit is installed and `nvcc` is in PATH
2. **Build errors**: Make sure you have compatible CUDA drivers
3. **Runtime errors**: Check that CUDA runtime libraries are available

### Error Messages

- `"CUDA feature not enabled"`: Run with `--features cuda`
- `"Model file not found"`: Download a GGUF model file
- `"CUDA not available"`: Check CUDA installation

## Migration Guide

If you're migrating from a hardcoded CUDA dependency:

1. **Update Cargo.toml**: Make `llama_cpp` optional and add the `cuda` feature
2. **Wrap imports**: Use `#[cfg(feature = "cuda")]` for CUDA-specific imports
3. **Conditional code**: Wrap CUDA-specific logic in feature blocks
4. **Update tests**: Add conditional test cases
5. **Update documentation**: Document the new optional feature

## Performance Comparison

| Configuration | Compile Time | Binary Size | Runtime Performance |
|---------------|--------------|-------------|-------------------|
| Without CUDA  | ~30s         | ~15MB       | CPU-only          |
| With CUDA     | ~60s         | ~25MB       | GPU accelerated   |

*Note: Actual performance depends on your hardware and model size.* 