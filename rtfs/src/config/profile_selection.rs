//! LLM profile expansion and auto-selection helpers.
//! Extracted from the live_interactive_assistant example for reuse & testing.

use std::collections::HashMap;

use crate::config::types::{AgentConfig, LlmProfile};

/// Metadata captured for synthetic or explicit profiles when available.
#[derive(Debug, Clone)]
pub struct ProfileMeta {
    pub prompt_cost: Option<f64>,
    pub completion_cost: Option<f64>,
    pub quality: Option<String>,
    pub notes: Option<String>,
    pub from_set: Option<String>,
}

/// Expand explicit profiles plus model_sets into a flat list of profiles.
/// Synthetic profile names take the form `<set>:<spec>`.
pub fn expand_profiles(
    cfg: &AgentConfig,
) -> (Vec<LlmProfile>, HashMap<String, ProfileMeta>, String) {
    let mut out: Vec<LlmProfile> = Vec::new();
    let mut meta: HashMap<String, ProfileMeta> = HashMap::new();
    let mut lines: Vec<String> = Vec::new();
    if let Some(llm_cfg) = &cfg.llm_profiles {
        for p in &llm_cfg.profiles {
            lines.push(format!(
                "explicit:{} provider={} model={}",
                p.name, p.provider, p.model
            ));
            out.push(p.clone());
        }
        if let Some(sets) = &llm_cfg.model_sets {
            for set in sets {
                for spec in &set.models {
                    let synthetic_name = format!("{}:{}", set.name, spec.name);
                    let profile = LlmProfile {
                        name: synthetic_name.clone(),
                        provider: set.provider.clone(),
                        model: spec.model.clone(),
                        base_url: set.base_url.clone(),
                        api_key_env: set.api_key_env.clone(),
                        api_key: set.api_key.clone(),
                        temperature: None,
                        max_tokens: spec.max_output_tokens,
                    };
                    out.push(profile);
                    meta.insert(
                        synthetic_name.clone(),
                        ProfileMeta {
                            prompt_cost: spec.max_prompt_cost_per_1k,
                            completion_cost: spec.max_completion_cost_per_1k,
                            quality: spec.quality.clone(),
                            notes: spec.notes.clone(),
                            from_set: Some(set.name.clone()),
                        },
                    );
                    lines.push(format!("set:{} spec={} provider={} model={} quality={:?} prompt_cost={:?} completion_cost={:?}", set.name, spec.name, set.provider, spec.model, spec.quality, spec.max_prompt_cost_per_1k, spec.max_completion_cost_per_1k));
                }
            }
        }
    }
    let rationale = if lines.is_empty() {
        "no llm profiles configured".to_string()
    } else {
        lines.join("\n")
    };
    (out, meta, rationale)
}

/// Rank helper for quality tiers.
pub fn quality_rank(q: &str) -> i32 {
    match q.to_lowercase().as_str() {
        "reasoning" => 7,
        "premium" => 6,
        "quality" | "high" => 5,
        "balanced" | "standard" => 4,
        "speed" | "fast" => 3,
        "basic" => 2,
        _ => 1,
    }
}

/// Auto-select a profile by budget and minimum quality constraints.
pub fn auto_select_model<'a>(
    profiles: &'a [LlmProfile],
    meta: &'a HashMap<String, ProfileMeta>,
    prompt_budget: Option<f64>,
    completion_budget: Option<f64>,
    min_quality: Option<&str>,
) -> (Option<&'a LlmProfile>, String) {
    let mut candidates: Vec<(&LlmProfile, &ProfileMeta)> = profiles
        .iter()
        .filter_map(|p| meta.get(&p.name).map(|m| (p, m)))
        .collect();

    let mut rationale_lines: Vec<String> = Vec::new();
    rationale_lines.push(format!("initial_candidates={}", candidates.len()));

    candidates.retain(|(_p, m)| {
        let prompt_ok = if let Some(b) = prompt_budget {
            m.prompt_cost.map(|c| c <= b).unwrap_or(false)
        } else {
            true
        };
        let completion_ok = if let Some(b) = completion_budget {
            m.completion_cost.map(|c| c <= b).unwrap_or(false)
        } else {
            true
        };
        let keep = prompt_ok && completion_ok;
        if !keep {
            rationale_lines.push(format!(
                "filtered:cost {} prompt_cost={:?} completion_cost={:?}",
                _p.name, m.prompt_cost, m.completion_cost
            ));
        }
        prompt_ok && completion_ok
    });

    if let Some(q) = min_quality {
        let min_rank = quality_rank(q);
        candidates.retain(|(p, m)| {
            let ok = m
                .quality
                .as_ref()
                .map(|qq| quality_rank(qq) >= min_rank)
                .unwrap_or(false);
            if !ok {
                rationale_lines.push(format!(
                    "filtered:quality {} quality={:?}",
                    p.name, m.quality
                ));
            }
            ok
        });
    }
    if candidates.is_empty() {
        return (None, rationale_lines.join("\n"));
    }

    candidates.sort_by(|a, b| {
        let qa = a.1.quality.as_ref().map(|q| quality_rank(q)).unwrap_or(0);
        let qb = b.1.quality.as_ref().map(|q| quality_rank(q)).unwrap_or(0);
        qb.cmp(&qa).then_with(|| {
            let ca = a.1.prompt_cost.unwrap_or(f64::MAX) + a.1.completion_cost.unwrap_or(f64::MAX);
            let cb = b.1.prompt_cost.unwrap_or(f64::MAX) + b.1.completion_cost.unwrap_or(f64::MAX);
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    let chosen = candidates[0].0;
    let cmeta = meta.get(&chosen.name);
    if let Some(m) = cmeta {
        rationale_lines.push(format!(
            "chosen:{} quality={:?} prompt_cost={:?} completion_cost={:?}",
            chosen.name, m.quality, m.prompt_cost, m.completion_cost
        ));
    }
    (Some(chosen), rationale_lines.join("\n"))
}
