//! Cached metadata from the [OpenRouter Models API](https://openrouter.ai/docs/guides/overview/models):
//! `context_length` and per-token pricing for budget / CLI estimates.
//!
//! Fetches `GET https://openrouter.ai/api/v1/models` (no API key required for the public list).
//! Disable with env `AUTONOETIC_OPENROUTER_CATALOG=0` or override URL with
//! `AUTONOETIC_OPENROUTER_MODELS_URL`.

use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const DEFAULT_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
const CACHE_TTL: Duration = Duration::from_secs(3600);

fn catalog_disabled() -> bool {
    matches!(
        std::env::var("AUTONOETIC_OPENROUTER_CATALOG")
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("0" | "false" | "no" | "off")
    )
}

fn models_url() -> String {
    std::env::var("AUTONOETIC_OPENROUTER_MODELS_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODELS_URL.to_string())
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelRow>,
}

#[derive(Debug, Deserialize)]
struct ModelRow {
    id: String,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    pricing: Option<PricingRow>,
}

#[derive(Debug, Deserialize, Default)]
struct PricingRow {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

#[derive(Debug, Clone)]
struct ModelEntry {
    context_length: Option<u32>,
    prompt_usd_per_token: f64,
    completion_usd_per_token: f64,
}

#[derive(Debug)]
struct CatalogState {
    by_id: HashMap<String, ModelEntry>,
    fetched_at: Option<Instant>,
}

/// In-memory cache of OpenRouter model metadata (context window + pricing).
#[derive(Debug, Clone)]
pub struct OpenRouterCatalog {
    client: reqwest::Client,
    models_url: String,
    inner: std::sync::Arc<tokio::sync::RwLock<CatalogState>>,
}

impl OpenRouterCatalog {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            models_url: models_url(),
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(CatalogState {
                by_id: HashMap::new(),
                fetched_at: None,
            })),
        }
    }

    /// Refresh the catalog if empty or stale. No-op when disabled via env.
    pub async fn refresh_if_needed(&self) -> anyhow::Result<()> {
        if catalog_disabled() {
            return Ok(());
        }
        let need_fetch = {
            let g = self.inner.read().await;
            g.by_id.is_empty()
                || g
                    .fetched_at
                    .map(|t| t.elapsed() > CACHE_TTL)
                    .unwrap_or(true)
        };
        if !need_fetch {
            return Ok(());
        }
        self.fetch().await
    }

    async fn fetch(&self) -> anyhow::Result<()> {
        let resp = self
            .client
            .get(&self.models_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "OpenRouter models HTTP {} from {}",
                resp.status(),
                self.models_url
            );
        }
        let parsed: ModelsResponse = resp.json().await?;
        let mut by_id = HashMap::with_capacity(parsed.data.len());
        for row in parsed.data {
            let prompt = row
                .pricing
                .as_ref()
                .and_then(|p| p.prompt.as_ref())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let completion = row
                .pricing
                .as_ref()
                .and_then(|p| p.completion.as_ref())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let context_length = row
                .context_length
                .and_then(|n| u32::try_from(n).ok());
            by_id.insert(
                row.id.clone(),
                ModelEntry {
                    context_length,
                    prompt_usd_per_token: prompt,
                    completion_usd_per_token: completion,
                },
            );
        }
        let mut w = self.inner.write().await;
        w.by_id = by_id;
        w.fetched_at = Some(Instant::now());
        tracing::info!(
            target: "autonoetic.openrouter",
            url = %self.models_url,
            models = w.by_id.len(),
            "OpenRouter models catalog refreshed"
        );
        Ok(())
    }

    /// Context length in tokens for a model id (e.g. `google/gemini-2.5-flash`), if known.
    pub async fn context_length_for_model(&self, model_id: &str) -> Option<u32> {
        if catalog_disabled() {
            return None;
        }
        let _ = self.refresh_if_needed().await;
        self.inner
            .read()
            .await
            .by_id
            .get(model_id)
            .and_then(|m| m.context_length)
    }

    /// Estimated USD cost for this completion using OpenRouter `pricing` (prompt + completion per token).
    pub async fn estimate_cost_usd(
        &self,
        model_id: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Option<f64> {
        if catalog_disabled() {
            return None;
        }
        let _ = self.refresh_if_needed().await;
        let g = self.inner.read().await;
        let entry = g.by_id.get(model_id)?;
        let cost = input_tokens as f64 * entry.prompt_usd_per_token
            + output_tokens as f64 * entry.completion_usd_per_token;
        Some(cost)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pricing_strings() {
        let p: PricingRow = serde_json::from_value(serde_json::json!({
            "prompt": "0.00000025",
            "completion": "0.0000005"
        }))
        .unwrap();
        assert_eq!(p.prompt.as_deref().unwrap().parse::<f64>().unwrap(), 0.00000025);
        assert_eq!(
            p.completion.as_deref().unwrap().parse::<f64>().unwrap(),
            0.0000005
        );
    }
}
