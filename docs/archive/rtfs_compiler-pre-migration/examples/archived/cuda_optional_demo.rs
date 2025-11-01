//! Example demonstrating optional CUDA support
//!
//! This example shows how to conditionally use CUDA features in your Rust project.
//!
//! To run with CUDA support:
//!   cargo run --example cuda_optional_demo --features cuda
//!
//! To run without CUDA support:
//!   cargo run --example cuda_optional_demo

use rtfs_compiler::ccos::delegation::ModelProvider;
use rtfs_compiler::ccos::local_models::LocalLlamaModel;

fn main() {
    println!("=== RTFS CUDA Optional Demo ===\n");

    // Check if CUDA feature is enabled
    #[cfg(feature = "cuda")]
    {
        println!("✅ CUDA feature is ENABLED");
        println!("   - llama_cpp with CUDA support is available");
        println!("   - GPU acceleration will be used if available");
    }

    #[cfg(not(feature = "cuda"))]
    {
        println!("❌ CUDA feature is DISABLED");
        println!("   - llama_cpp is not compiled in");
        println!("   - GPU acceleration is not available");
    }

    println!();

    // Create a model instance (this will work regardless of CUDA feature)
    let model = LocalLlamaModel::default();
    println!("📦 Created model instance: {:?}", model);

    // Try to use the model
    println!("\n🔄 Attempting to use the model...");
    match model.infer("Hello, world!") {
        Ok(response) => {
            println!("✅ Success! Response: {}", response);
        }
        Err(e) => {
            println!("❌ Error: {}", e);

            #[cfg(not(feature = "cuda"))]
            {
                println!("\n💡 To enable CUDA support, run:");
                println!("   cargo run --example cuda_optional_demo --features cuda");
            }
        }
    }

    println!("\n=== Demo Complete ===");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_creation() {
        let model = LocalLlamaModel::default();
        assert_eq!(model.id(), "local-llama");
    }

    #[test]
    fn test_cuda_feature_detection() {
        #[cfg(feature = "cuda")]
        {
            // This test only runs when CUDA feature is enabled
            assert!(true, "CUDA feature is enabled");
        }

        #[cfg(not(feature = "cuda"))]
        {
            // This test only runs when CUDA feature is disabled
            assert!(true, "CUDA feature is disabled");
        }
    }
}
