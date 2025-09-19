use async_trait::async_trait;
use std::sync::Arc;

use crate::ccos::types::{Intent, Plan};
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::runtime::error::RuntimeError;
use serde_json::Value as JsonValue;
use super::llm_provider::{LlmProviderFactory, LlmProviderConfig};

/// Result of plan generation, optionally carrying an IR echo for evaluation.
pub struct PlanGenerationResult {
    pub plan: Plan,
    pub ir_json: Option<serde_json::Value>,
    pub diagnostics: Option<String>,
}

#[async_trait(?Send)]
pub trait PlanGenerationProvider {
    async fn generate_plan(
        &self,
        intent: &Intent,
        marketplace: Arc<CapabilityMarketplace>,
    ) -> Result<PlanGenerationResult, RuntimeError>;
}

/// Deterministic stub provider used in tests and demos.
/// Generates a simple two-step plan using stdlib capabilities if available.
pub struct StubPlanGenerationProvider;

/// Very small RTFS → JSON IR canonicalizer for simple (do (step ... (call ...))) forms.
/// This is intentionally minimal to support stub/demo plans and produce a comparable shape.
fn canonicalize_rtfs_to_ir_json(rtfs: &str) -> JsonValue {
    // Parse line-by-line looking for (step "Name" (call CAP ...))
    let mut steps: Vec<JsonValue> = Vec::new();
    for line in rtfs.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("(step ") { continue; }

        // Extract name between the first pair of quotes
        let name = trimmed
            .splitn(2, '"')
            .nth(1)
            .and_then(|s| s.splitn(2, '"').next())
            .unwrap_or("")
            .to_string();

        // Find capability token after "(call"
        let cap = if let Some(call_idx) = trimmed.find("(call ") {
            let after = &trimmed[call_idx + 6..]; // after "(call "
            let mut tok = after.split_whitespace().next().unwrap_or("");
            // Normalize quotes for string caps
            if tok.starts_with('"') {
                tok = tok.trim_matches('"');
            }
            tok.to_string()
        } else { String::new() };

        // Naive args extraction for two common shapes used in stubs:
        // 1) map literal: {:message "hi"}
        // 2) positional integers: 2 3
        let mut args_json: JsonValue = JsonValue::Null;
        if let Some(call_idx) = trimmed.find("(call ") {
            let after = &trimmed[call_idx + 6..];
            // Drop capability token
            let after_cap = after.splitn(2, ' ').nth(1).unwrap_or("").trim();
            if after_cap.starts_with('{') {
                // Extremely limited map parser for {:k "v"}
                if after_cap.contains(":message") {
                    if let Some(q1) = after_cap.find('"') {
                        if let Some(q2) = after_cap[q1+1..].find('"') {
                            let val = &after_cap[q1+1..q1+1+q2];
                            args_json = serde_json::json!({"message": val});
                        }
                    }
                } else {
                    args_json = JsonValue::Null; // unknown shape → null
                }
            } else {
                // Collect positional ints if any
                let mut arr: Vec<JsonValue> = Vec::new();
                for tok in after_cap.split_whitespace() {
                    // Stop at closing parens
                    let t = tok.trim_end_matches(')').trim_end_matches(')');
                    if let Ok(n) = t.parse::<i64>() { arr.push(JsonValue::from(n)); }
                }
                if !arr.is_empty() { args_json = JsonValue::Array(arr); }
            }
        }

        let id = format!("s{}", steps.len() + 1);
        let step = serde_json::json!({
            "id": id,
            "name": name,
            "capability": cap,
            "args": args_json,
            // Sequential dependency assumption for simple (do ...) bodies
            "deps": if steps.is_empty() { serde_json::json!([]) } else { serde_json::json!([format!("s{}", steps.len())]) }
        });
        steps.push(step);
    }
    serde_json::json!({"steps": steps})
}

#[async_trait(?Send)]
impl PlanGenerationProvider for StubPlanGenerationProvider {
    async fn generate_plan(
        &self,
        intent: &Intent,
        _marketplace: Arc<CapabilityMarketplace>,
    ) -> Result<PlanGenerationResult, RuntimeError> {
        // Minimal RTFS body; governance will still validate.
      let body = r#"(do
  (step "Greet" (call :ccos.echo {:message "hi"}))
  (step "Add" (call :ccos.math.add 2 3)))"#;
        // Create a simple JSON IR mirroring the steps
        let ir = serde_json::json!({
            "steps": [
                {
                    "id": "s1",
                    "name": "Greet",
                    "capability": ":ccos.echo",
                    "args": {"message": "hi"},
                    "deps": []
                },
                {
                    "id": "s2",
                    "name": "Add",
                    "capability": ":ccos.math.add",
                    "args": [2, 3],
                    "deps": ["s1"]
                }
            ]
        });

        // Create plan bound to the provided intent
        let plan = Plan::new_rtfs(body.to_string(), vec![intent.intent_id.clone()]);

        // Canonicalize RTFS to a comparable IR shape and compute a simple equivalence signal
        let canon = canonicalize_rtfs_to_ir_json(body);
        let caps_from = |j: &JsonValue| -> Vec<String> {
            j.get("steps").and_then(|v| v.as_array()).unwrap_or(&vec![])
                .iter()
                .map(|s| s.get("capability").and_then(|c| c.as_str()).unwrap_or("").to_string())
                .collect()
        };
        let caps_ir = caps_from(&ir);
        let caps_canon = caps_from(&canon);
        let eq_len = ir.get("steps").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0)
            == canon.get("steps").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        let eq_caps = caps_ir == caps_canon;
        let equivalent = eq_len && eq_caps;
        let diagnostics = Some(format!(
            "stub: direct RTFS generated; IR provided for comparison; ir-equivalence: {} ; steps={} ; caps={}",
            equivalent,
            caps_ir.len(),
            caps_ir.join(",")
        ));

        Ok(PlanGenerationResult { plan, ir_json: Some(ir), diagnostics })
    }
}

/// LLM-backed plan generation provider using the reduced-grammar RTFS prompt.
pub struct LlmRtfsPlanGenerationProvider {
    pub config: LlmProviderConfig,
}

impl LlmRtfsPlanGenerationProvider {
    pub fn new(config: LlmProviderConfig) -> Self { Self { config } }
}

#[async_trait(?Send)]
impl PlanGenerationProvider for LlmRtfsPlanGenerationProvider {
    async fn generate_plan(
        &self,
        intent: &Intent,
        _marketplace: Arc<CapabilityMarketplace>,
    ) -> Result<PlanGenerationResult, RuntimeError> {
        // Build a minimal StorableIntent shim to call the LLM provider API
        use crate::ccos::types::{StorableIntent, IntentStatus, TriggerSource, GenerationContext};
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let storable = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: "".to_string(),
            goal: intent.goal.clone(),
            constraints: intent
                .constraints
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            preferences: intent
                .preferences
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            success_criteria: intent.success_criteria.as_ref().map(|v| format!("{}", v)),
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "llm-rtfs-provider-1.0".to_string(),
                generation_timestamp: now,
                input_context: std::collections::HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 0,
            created_at: now,
            updated_at: now,
            metadata: std::collections::HashMap::new(),
        };

        let provider = LlmProviderFactory::create_provider(self.config.clone()).await?;
        let plan = provider.generate_plan(&storable, None).await?;

        // Attach basic diagnostics (provider name + model)
        let info = provider.get_info();
        let mode = if std::env::var("RTFS_FULL_PLAN").map(|v| v == "1").unwrap_or(false) {
            "full-plan"
        } else {
            "reduced-grammar-rtfs"
        };
        let diagnostics = Some(format!(
            "llm-provider: {} model={} ; mode={}",
            info.name, info.model, mode
        ));

        Ok(PlanGenerationResult { plan, ir_json: None, diagnostics })
    }
}
