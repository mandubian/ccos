use rtfs_compiler::config::{auto_select_model, expand_profiles, quality_rank, AgentConfig};

fn sample_config_json() -> &'static str {
    r#"{
  "version": "1",
  "agent_id": "test-agent",
  "profile": "dev",
  "orchestrator": {"isolation": {"mode": "wasm", "fs": {"ephemeral": true, "mounts": {}}}, "dlp": {"enabled": false, "policy": "lenient"}},
  "network": {"enabled": false, "egress": {"via": "none", "allow_domains": [], "mtls": false, "tls_pins": []}},
  "microvm": null,
  "capabilities": {"http": {"enabled": false, "egress": {"allow_domains": [], "mtls": false}}, "fs": {"enabled": false}, "llm": {"enabled": false}},
  "governance": {"policies": {"default": {"risk_tier": "low", "requires_approvals": 0, "budgets": {"max_cost_usd": 0.0, "token_budget": 0.0}}}, "keys": {"verify": ""}},
  "causal_chain": {"storage": {"mode": "in_memory"}, "anchor": {"enabled": false}},
  "marketplace": {"registry_paths": []},
  "delegation": {"enabled": false, "model": null, "temperature": null, "max_tokens": null},
  "features": [],
  "llm_profiles": {
    "default": "openai-gpt4o",
    "profiles": [
      { "name": "openai-gpt4o", "provider": "openai", "model": "gpt-4o", "api_key_env": "OPENAI_API_KEY" }
    ],
    "model_sets": [
      { "name": "foundation", "provider": "openai", "api_key_env": "OPENAI_API_KEY",
        "models": [
          { "name": "fast", "model": "gpt-4o-mini", "max_prompt_cost_per_1k": 0.15, "max_completion_cost_per_1k": 0.60, "quality": "speed" },
          { "name": "balanced", "model": "gpt-4o", "max_prompt_cost_per_1k": 0.50, "max_completion_cost_per_1k": 1.50, "quality": "balanced" },
          { "name": "reasoning", "model": "o4-mini", "max_prompt_cost_per_1k": 3.00, "max_completion_cost_per_1k": 15.00, "quality": "reasoning" }
        ]
      }
    ]
  }
}"#
}

#[test]
fn test_expand_profiles_includes_synthetic() {
    let cfg: AgentConfig = serde_json::from_str(sample_config_json()).expect("parse config");
    let (profiles, meta, _rationale) = expand_profiles(&cfg);
    // explicit + 3 synthetic
    assert_eq!(profiles.len(), 1 + 3);
    assert!(profiles.iter().any(|p| p.name == "foundation:fast"));
    assert!(profiles.iter().any(|p| p.name == "foundation:balanced"));
    assert!(profiles.iter().any(|p| p.name == "foundation:reasoning"));
    // meta contains only synthetic entries (cost annotated)
    assert!(
        meta.get("foundation:balanced")
            .unwrap()
            .prompt_cost
            .unwrap()
            - 0.50
            < 1e-9
    );
}

#[test]
fn test_auto_select_budget_and_quality_filters() {
    let cfg: AgentConfig = serde_json::from_str(sample_config_json()).expect("parse config");
    let (profiles, meta, _rationale) = expand_profiles(&cfg);

    // Tight budget should pick 'fast'
    let (chosen, _why) = auto_select_model(&profiles, &meta, Some(0.20), Some(0.70), None);
    let chosen = chosen.expect("expected fast candidate");
    assert_eq!(chosen.name, "foundation:fast");
    // rationale should mention chosen
    let (_c, rationale_fast) = auto_select_model(&profiles, &meta, Some(0.20), Some(0.70), None);
    assert!(
        rationale_fast.contains("chosen:foundation:fast"),
        "rationale missing chosen fast line: {}",
        rationale_fast
    );

    // Higher budget but min quality balanced should pick 'balanced'
    let (chosen, _why) =
        auto_select_model(&profiles, &meta, Some(0.60), Some(2.00), Some("balanced"));
    let chosen = chosen.expect("expected balanced candidate");
    assert_eq!(chosen.name, "foundation:balanced");
    let (_c, rationale_balanced) =
        auto_select_model(&profiles, &meta, Some(0.60), Some(2.00), Some("balanced"));
    assert!(rationale_balanced.contains("chosen:foundation:balanced"));

    // Min quality reasoning should pick reasoning even though cost higher
    let (chosen, _why) =
        auto_select_model(&profiles, &meta, Some(5.0), Some(20.0), Some("reasoning"));
    let chosen = chosen.expect("expected reasoning candidate");
    assert_eq!(chosen.name, "foundation:reasoning");
    let (_c, rationale_reasoning) =
        auto_select_model(&profiles, &meta, Some(5.0), Some(20.0), Some("reasoning"));
    assert!(rationale_reasoning.contains("chosen:foundation:reasoning"));

    // Overly strict budget leads to no selection
    let (none, _why) = auto_select_model(&profiles, &meta, Some(0.01), Some(0.01), None);
    assert!(none.is_none());
    let (_none, rationale_none) = auto_select_model(&profiles, &meta, Some(0.01), Some(0.01), None);
    assert!(rationale_none.contains("initial_candidates"));
}

#[test]
fn test_policy_shorthand_deserialization() {
    let toml_cfg = r#"version = "1"
agent_id = "short-policy"
profile = "dev"

[governance.policies]
default = "allow"

[governance.keys]
verify = "abc"
"#;
    let cfg: AgentConfig = toml::from_str(toml_cfg).expect("parse toml with shorthand policy");
    let pol = cfg
        .governance
        .policies
        .get("default")
        .expect("default policy present");
    assert_eq!(pol.risk_tier, "low");
    assert_eq!(pol.requires_approvals, 0);
    assert!(pol.budgets.max_cost_usd >= 1.0);
}

#[test]
fn test_policy_shorthand_moderate_and_strict() {
    let toml_cfg = r#"version = "1"
agent_id = "multi-policy"
profile = "dev"

[governance.policies]
build = "moderate"
deploy = "strict"

[governance.keys]
verify = "k"
"#;
    let cfg: AgentConfig = toml::from_str(toml_cfg).expect("parse multi policy");
    let moderate = cfg.governance.policies.get("build").unwrap();
    assert_eq!(moderate.risk_tier, "medium");
    assert_eq!(moderate.requires_approvals, 1);
    let strict = cfg.governance.policies.get("deploy").unwrap();
    assert_eq!(strict.risk_tier, "high");
    assert_eq!(strict.requires_approvals, 2);
}

#[test]
fn test_quality_rank_ordering() {
    assert!(quality_rank("reasoning") > quality_rank("balanced"));
    assert!(quality_rank("balanced") > quality_rank("speed"));
    assert!(quality_rank("speed") > quality_rank("basic"));
}

#[test]
fn test_policy_shorthand_invalid() {
    let toml_cfg = r#"version = "1"
agent_id = "invalid-policy"
profile = "dev"

[governance.policies]
weird = "ultra"

[governance.keys]
verify = "k"
"#;
    let err =
        toml::from_str::<AgentConfig>(toml_cfg).expect_err("expected failure on invalid shorthand");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown policy shorthand 'ultra' (expected allow|moderate|strict)"),
        "unexpected error message: {}",
        msg
    );
}
