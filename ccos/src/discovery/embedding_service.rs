//! Embedding Service for Semantic Matching
//!
//! Provides vector embeddings for semantic similarity calculation.
//! Supports both remote (OpenRouter) and local embedding models.

use crate::discovery::config::DiscoveryConfig;
use serde::Deserialize;
use std::collections::HashMap;

// Use RuntimeError from rtfs, but check if it's available
type RuntimeError = rtfs::runtime::error::RuntimeError;
type RuntimeResult<T> = Result<T, RuntimeError>;

/// Embedding provider configuration
#[derive(Debug, Clone)]
pub enum EmbeddingProvider {
    /// OpenRouter API (supports various embedding models)
    OpenRouter {
        api_key: String,
        model: String, // e.g., "text-embedding-ada-002", "text-embedding-3-small"
    },
    /// Local embedding model (via HTTP endpoint, e.g., Ollama)
    Local {
        base_url: String, // e.g., "http://localhost:11434/api/embeddings"
        model: String,    // e.g., "nomic-embed-text"
    },
}

impl EmbeddingProvider {
    /// Create provider from environment variables and optional discovery configuration
    /// Priority: LOCAL_EMBEDDING_URL (Ollama) > OPENROUTER_API_KEY (remote)
    pub fn from_env(config: Option<&DiscoveryConfig>) -> Option<Self> {
        // Try local model first (Ollama - cheaper and faster)
        if let Ok(base_url) = std::env::var("LOCAL_EMBEDDING_URL") {
            let model = std::env::var("LOCAL_EMBEDDING_MODEL")
                .ok()
                .or_else(|| config.and_then(|c| c.local_embedding_model.clone()))
                .unwrap_or_else(|| "nomic-embed-text".to_string());
            return Some(EmbeddingProvider::Local { base_url, model });
        }

        // Fallback to OpenRouter
        if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY") {
            let model = std::env::var("EMBEDDING_MODEL")
                .ok()
                .or_else(|| config.and_then(|c| c.embedding_model.clone()))
                .unwrap_or_else(|| "text-embedding-ada-002".to_string());
            return Some(EmbeddingProvider::OpenRouter { api_key, model });
        }

        None
    }
}

/// Embedding service for generating vector embeddings
pub struct EmbeddingService {
    provider: EmbeddingProvider,
    client: reqwest::Client,
    cache: HashMap<String, Vec<f32>>, // Simple in-memory cache
}

impl EmbeddingService {
    /// Create a new embedding service
    pub fn new(provider: EmbeddingProvider) -> Self {
        Self {
            provider,
            client: reqwest::Client::new(),
            cache: HashMap::new(),
        }
    }

    /// Create from environment variables (returns None if not configured)
    pub fn from_env() -> Option<Self> {
        Self::from_settings(None)
    }

    /// Create from discovery configuration + environment overrides
    pub fn from_settings(config: Option<&DiscoveryConfig>) -> Option<Self> {
        EmbeddingProvider::from_env(config).map(Self::new)
    }

    /// Get a description of the current provider (for logging)
    pub fn provider_description(&self) -> String {
        match &self.provider {
            EmbeddingProvider::OpenRouter { model, .. } => format!("OpenRouter ({})", model),
            EmbeddingProvider::Local { base_url, model } => {
                format!("Local ({} @ {})", model, base_url)
            }
        }
    }

    /// Generate embedding for a text string
    pub async fn embed(&mut self, text: &str) -> RuntimeResult<Vec<f32>> {
        // Check cache first
        if let Some(cached) = self.cache.get(text) {
            return Ok(cached.clone());
        }

        // Generate embedding based on provider
        let embedding = match &self.provider {
            EmbeddingProvider::OpenRouter { api_key, model } => {
                self.embed_via_openrouter(api_key, model, text).await?
            }
            EmbeddingProvider::Local { base_url, model } => {
                self.embed_via_local(base_url, model, text).await?
            }
        };

        // Cache the result
        self.cache.insert(text.to_string(), embedding.clone());

        Ok(embedding)
    }

    /// Generate embedding via OpenRouter API
    async fn embed_via_openrouter(
        &self,
        api_key: &str,
        model: &str,
        text: &str,
    ) -> RuntimeResult<Vec<f32>> {
        let url = "https://openrouter.ai/api/v1/embeddings";

        let request_body = serde_json::json!({
            "model": model,
            "input": text,
        });

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header(
                "HTTP-Referer",
                std::env::var("OPENROUTER_HTTP_REFERER")
                    .unwrap_or_else(|_| "https://github.com/mandubian/ccos".to_string()),
            )
            .header(
                "X-Title",
                std::env::var("OPENROUTER_TITLE")
                    .unwrap_or_else(|_| "CCOS Embedding Service".to_string()),
            )
            .json(&request_body)
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("OpenRouter API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!(
                "OpenRouter API error ({}): {}",
                status, error_text
            )));
        }

        let result: OpenRouterEmbeddingResponse = response.json().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse OpenRouter response: {}", e))
        })?;

        // Extract embedding vector
        if result.data.is_empty() {
            return Err(RuntimeError::Generic(
                "No embedding data in response".to_string(),
            ));
        }

        Ok(result.data[0].embedding.clone())
    }

    /// Generate embedding via local model (e.g., Ollama)
    async fn embed_via_local(
        &self,
        base_url: &str,
        model: &str,
        text: &str,
    ) -> RuntimeResult<Vec<f32>> {
        let url = format!("{}/embeddings", base_url);

        let request_body = serde_json::json!({
            "model": model,
            "prompt": text,
        });

        let response = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                RuntimeError::Generic(format!("Local embedding API request failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!(
                "Local embedding API error ({}): {}",
                status, error_text
            )));
        }

        let result: LocalEmbeddingResponse = response.json().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse local API response: {}", e))
        })?;

        Ok(result.embedding)
    }

    /// Calculate cosine similarity between two embeddings
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            (dot_product / (norm_a * norm_b)) as f64
        }
    }
}

/// OpenRouter embedding response format
#[derive(Debug, Deserialize)]
struct OpenRouterEmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// Local embedding response format (Ollama-compatible)
#[derive(Debug, Deserialize)]
struct LocalEmbeddingResponse {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        // Test identical vectors
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((EmbeddingService::cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        // Test orthogonal vectors
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((EmbeddingService::cosine_similarity(&a, &b) - 0.0).abs() < 0.001);

        // Test similar vectors
        let a = vec![1.0, 1.0, 0.0];
        let b = vec![1.0, 1.0, 0.001];
        let similarity = EmbeddingService::cosine_similarity(&a, &b);
        assert!(
            similarity > 0.9,
            "Similar vectors should have high similarity"
        );
    }

    #[tokio::test]
    #[ignore] // Requires actual API keys
    async fn test_openrouter_embedding() {
        if let Some(mut service) = EmbeddingService::from_env() {
            let embedding = service.embed("test text").await.unwrap();
            assert!(!embedding.is_empty());
            assert!(embedding.len() > 100); // Most embedding models produce >100 dim vectors
        }
    }
}
