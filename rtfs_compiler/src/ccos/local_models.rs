//! Local Model Providers using llama-cpp
//! 
//! This module provides realistic local model implementations using efficient
//! quantized LLMs that can run on GPU with llama-cpp.

use crate::ccos::delegation::ModelProvider;
use llama_cpp::{LlamaModel, LlamaParams, SessionParams, standard_sampler::StandardSampler};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use futures::executor::block_on;
use tokio::task;

/// Realistic local model provider using llama-cpp
pub struct LocalLlamaModel {
    id: &'static str,
    model_path: String,
    model: Arc<Mutex<Option<LlamaModel>>>,
}

impl LocalLlamaModel {
    /// Create a new local llama model provider
    pub fn new(
        id: &'static str,
        model_path: &str,
        _params: Option<LlamaParams>,
    ) -> Self {
        Self {
            id,
            model_path: model_path.to_string(),
            model: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize the model (lazy loading)
    async fn ensure_loaded(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut model_guard = self.model.lock().await;
        
        if model_guard.is_none() {
            // Check if model file exists
            if !Path::new(&self.model_path).exists() {
                return Err(format!("Model file not found: {}", self.model_path).into());
            }

            // Load the model (once per process)
            println!("[LocalLlamaModel] 🔄 Loading model from '{}'. This should appear only once.", self.model_path);
            let model = LlamaModel::load_from_file(&self.model_path, LlamaParams::default())?;
            *model_guard = Some(model);
        }

        Ok(())
    }

    /// Create a default model using a common efficient model
    pub fn default() -> Self {
        // Use a path that can be overridden via environment variable
        let model_path = std::env::var("RTFS_LOCAL_MODEL_PATH")
            .unwrap_or_else(|_| "models/phi-2.gguf".to_string());
        
        Self::new("local-llama", &model_path, None)
    }

    /// Create a model optimized for RTFS function calls
    pub fn rtfs_optimized() -> Self {
        let params = LlamaParams::default();

        let model_path = std::env::var("RTFS_LOCAL_MODEL_PATH")
            .unwrap_or_else(|_| "models/phi-2.gguf".to_string());

        Self::new("rtfs-llama", &model_path, Some(params))
    }

    /// Core async inference logic shared by both sync entrypoints.
    async fn infer_async(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Ensure model is loaded
        self.ensure_loaded().await?;

        // Get the model
        let model_guard = self.model.lock().await;
        let model = model_guard.as_ref().ok_or("Model not loaded")?;

        // Create a session
        let mut session = model.create_session(SessionParams::default())?;

        // Format the prompt for RTFS function calls
        let formatted_prompt = format!(
            "You are an RTFS function execution assistant. Given the following function arguments, provide a concise response that would be the result of executing the function.\n\nArguments: {}\n\nResponse:",
            prompt
        );

        // Advance context with the prompt
        session.advance_context(&formatted_prompt)?;

        // Generate response
        let mut response = String::new();
        let completions = session.start_completing_with(StandardSampler::default(), 256)?;
        let mut string_completions = completions.into_strings();
        for completion in string_completions {
            response.push_str(&completion);
        }

        Ok(response)
    }
}

impl ModelProvider for LocalLlamaModel {
    fn id(&self) -> &'static str {
        self.id
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // If we're already inside a Tokio runtime, reuse it; otherwise create one.
        match tokio::runtime::Handle::try_current() {
            Ok(_) => task::block_in_place(|| block_on(self.infer_async(prompt))),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(self.infer_async(prompt))
            }
        }
    }
}

impl std::fmt::Debug for LocalLlamaModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalLlamaModel")
            .field("id", &self.id)
            .field("model_path", &self.model_path)
            .field("model", &if self.model.try_lock().is_ok() { "Loaded" } else { "Not loaded" })
            .finish()
    }
}

/// Model downloader utility
pub struct ModelDownloader;

impl ModelDownloader {
    /// Download a common efficient model if not present
    pub async fn ensure_model_available(model_path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if Path::new(model_path).exists() {
            return Ok(());
        }

        // Create models directory
        let models_dir = Path::new("models");
        if !models_dir.exists() {
            std::fs::create_dir_all(models_dir)?;
        }

        // For now, just provide instructions
        println!("Model file not found: {}", model_path);
        println!("Please download a GGUF model file and place it at: {}", model_path);
        println!("Recommended models:");
        println!("  - Microsoft Phi-2 (efficient, good performance)");
        println!("  - Llama-2-7B-Chat (good balance)");
        println!("  - Mistral-7B-Instruct (excellent performance)");
        println!();
        println!("You can download from Hugging Face or use:");
        println!("  wget https://huggingface.co/TheBloke/phi-2-GGUF/resolve/main/phi-2.Q4_K_M.gguf -O {}", model_path);

        Err("Model file not found. Please download a GGUF model.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_llama_model_creation() {
        let model = LocalLlamaModel::new("test-model", "test.gguf", None);
        assert_eq!(model.id(), "test-model");
        assert_eq!(model.model_path, "test.gguf");
    }

    #[test]
    fn test_default_model_creation() {
        let model = LocalLlamaModel::default();
        assert_eq!(model.id(), "local-llama");
    }

    #[test]
    fn test_rtfs_optimized_model_creation() {
        let model = LocalLlamaModel::rtfs_optimized();
        assert_eq!(model.id(), "rtfs-llama");
    }
} 