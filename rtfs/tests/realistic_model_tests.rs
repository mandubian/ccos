// NOTE: This test requires CCOS integration for LocalLlamaModel
// Skipped in pure RTFS mode - local models are CCOS capabilities
#[cfg(feature = "ccos-integration")]
use rtfs::ccos::delegation::ModelProvider;
#[cfg(feature = "ccos-integration")]
use rtfs::ccos::local_models::LocalLlamaModel;
#[cfg(feature = "ccos-integration")]
use std::env;

#[test]
#[cfg(feature = "ccos-integration")]
fn test_realistic_llama_model_inference() {
    // Use the environment variable or default path
    let model_path =
        env::var("RTFS_LOCAL_MODEL_PATH").unwrap_or_else(|_| "models/phi-2.gguf".to_string());
    if !Path::new(&model_path).exists() {
        eprintln!(
            "[SKIP] test_realistic_llama_model_inference: Model file not found at {}",
            model_path
        );
        return;
    }

    let model = LocalLlamaModel::new("test-llama", &model_path, None);
    let prompt = "What is the capital of France?";
    let result = model.infer(prompt);
    match result {
        Ok(response) => {
            println!("Model response: {}", response.trim());
            assert!(
                !response.trim().is_empty(),
                "Model returned an empty response"
            );
        }
        Err(e) => {
            panic!("Model inference failed: {}", e);
        }
    }
}
