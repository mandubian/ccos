use std::sync::Arc;

use crate::ccos::agent::{AgentDescriptor, TrustTier, CostModel, LatencyStats, SuccessStats};
use crate::ccos::CCOS;
use crate::runtime::security::{RuntimeContext, SecurityLevel};
use crate::runtime::values::Value;

#[tokio::test]
async fn test_ccos_with_delegating_arbiter_stub_model() {
    // Enable DelegatingArbiter with deterministic stub model
    std::env::set_var("CCOS_USE_DELEGATING_ARBITER", "1");
    std::env::set_var("CCOS_DELEGATING_MODEL", "stub-model");

    // Build CCOS with defaults (registers echo and math.add capabilities)
    let ccos = CCOS::new().await.expect("failed to init CCOS");

    // Security context allowing the capabilities used by the stub plan
    let context = RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: vec![
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
        ].into_iter().collect(),
        ..RuntimeContext::pure()
    };

    // Run a natural language request through the full pipeline
    let request = "please perform a small delegated task";
    let result = ccos.process_request(request, &context).await.expect("process_request failed");

    assert!(result.success);
    match result.value {
        Value::String(s) => assert!(s.contains("stub done"), "unexpected final value: {}", s),
        v => panic!("unexpected result value: {:?}", v),
    }

    // The intent should be stored and searchable in the IntentGraph
    let ig = ccos.get_intent_graph();
    let ig_locked = ig.lock().expect("lock intent graph");
    let intents = ig_locked.find_relevant_intents("delegated");
    assert!(intents.len() >= 1, "expected at least one stored intent");
}

#[tokio::test]
async fn test_agent_registry_delegation_short_circuit() {
    std::env::set_var("CCOS_USE_DELEGATING_ARBITER", "1");
    std::env::set_var("CCOS_DELEGATING_MODEL", "deterministic-stub-model");

    let ccos = CCOS::new().await.expect("init ccos");

    // Register a high scoring agent covering goal keywords & constraints
    {
        let reg_arc = ccos.get_agent_registry();
        let mut reg = reg_arc.write().unwrap();
        reg.register(AgentDescriptor {
            agent_id: "competitive_agent".into(),
            kind: "planner".into(),
            skills: vec!["competitive".into(), "analysis".into()],
            supported_constraints: vec!["budget".into(), "data-locality".into()],
            trust_tier: TrustTier::T1Trusted,
            cost: CostModel { cost_per_call: 0.05, tokens_per_second: 100.0 },
            latency: LatencyStats { p50_ms: 120.0, p95_ms: 250.0 },
            success: SuccessStats { success_rate: 0.9, samples: 25 },
            provenance: None,
        });
    }

    let ctx = RuntimeContext { security_level: SecurityLevel::Controlled, allowed_capabilities: vec!["ccos.echo".into(), "ccos.math.add".into()].into_iter().collect(), ..RuntimeContext::pure() };

    let request = "Need competitive analysis of EU market, keep cost under $10 and respect EU data locality";

    // We only call natural_language_to_intent to see if it gets delegated without using LLM
    if let Some(da) = ccos.get_delegating_arbiter() {
        use crate::ccos::arbiter_engine::ArbiterEngine;
        let intent = da.natural_language_to_intent(request, None).await.expect("intent generation");
        // Assert metadata contains delegation markers
        assert!(intent.metadata.contains_key("delegation.selected_agent"), "delegation did not occur");
        assert_eq!(intent.name.unwrap_or_default(), "competitive_agent");
        assert!(intent.metadata.get("delegation.candidates").unwrap().as_string().unwrap().contains("competitive_agent"));
    } else {
        panic!("Delegating arbiter not enabled");
    }
}

#[tokio::test]
async fn test_delegation_env_threshold_overrides_config() {
    // Enable delegating arbiter
    std::env::set_var("CCOS_USE_DELEGATING_ARBITER", "1");
    std::env::set_var("CCOS_DELEGATING_MODEL", "stub-model");
    // Force a high threshold via env so delegation should NOT occur
    std::env::set_var("CCOS_DELEGATION_THRESHOLD", "0.99");

    let ccos = CCOS::new().await.expect("init ccos");

    // Register an agent that would normally score high (~>0.7) but below 0.99
    {
        let reg_arc = ccos.get_agent_registry();
        let mut reg = reg_arc.write().unwrap();
        reg.register(AgentDescriptor {
            agent_id: "high_agent".into(),
            kind: "planner".into(),
            skills: vec!["analysis".into(), "eu".into()],
            supported_constraints: vec!["budget".into(), "data-locality".into()],
            trust_tier: TrustTier::T1Trusted,
            cost: CostModel { cost_per_call: 0.01, tokens_per_second: 100.0 },
            latency: LatencyStats { p50_ms: 50.0, p95_ms: 100.0 },
            success: SuccessStats { success_rate: 0.9, samples: 40 },
            provenance: None,
        });
    }

    let request = "Provide EU market analysis under budget";
    if let Some(da) = ccos.get_delegating_arbiter() {
        use crate::ccos::arbiter_engine::ArbiterEngine;
        let intent = da.natural_language_to_intent(request, None).await.expect("intent");
        // Should not have delegated because env threshold too high
        assert!(!intent.metadata.contains_key("delegation.selected_agent"), "delegation should have been blocked by high threshold");
    } else { panic!("delegating arbiter missing"); }
}

#[tokio::test]
async fn test_delegation_min_skill_hits_enforced() {
    std::env::set_var("CCOS_USE_DELEGATING_ARBITER", "1");
    std::env::set_var("CCOS_DELEGATING_MODEL", "stub-model");
    std::env::set_var("CCOS_DELEGATION_MIN_SKILL_HITS", "3"); // require 3 hits

    let ccos = CCOS::new().await.expect("init ccos");
    {
        let reg_arc = ccos.get_agent_registry();
        let mut reg = reg_arc.write().unwrap();
        reg.register(AgentDescriptor {
            agent_id: "two_skill_agent".into(),
            kind: "planner".into(),
            skills: vec!["analysis".into(), "market".into()], // only 2 possible hits
            supported_constraints: vec!["budget".into()],
            trust_tier: TrustTier::T1Trusted,
            cost: CostModel { cost_per_call: 0.02, tokens_per_second: 80.0 },
            latency: LatencyStats { p50_ms: 70.0, p95_ms: 140.0 },
            success: SuccessStats { success_rate: 0.85, samples: 20 },
            provenance: None,
        });
    }
    let request = "Need market analysis under budget"; // will only yield 2 hits
    if let Some(da) = ccos.get_delegating_arbiter() {
        use crate::ccos::arbiter_engine::ArbiterEngine;
        let intent = da.natural_language_to_intent(request, None).await.expect("intent");
        assert!(!intent.metadata.contains_key("delegation.selected_agent"), "delegation should not occur due to min skill hits");
    } else { panic!("delegating arbiter missing"); }
}

#[tokio::test]
async fn test_delegation_disabled_flag_blocks_delegation() {
    std::env::set_var("CCOS_USE_DELEGATING_ARBITER", "1");
    std::env::set_var("CCOS_DELEGATING_MODEL", "stub-model");
    std::env::set_var("CCOS_DELEGATION_ENABLED", "0");

    let ccos = CCOS::new().await.expect("init ccos");
    {
        let reg_arc = ccos.get_agent_registry();
        let mut reg = reg_arc.write().unwrap();
        reg.register(AgentDescriptor {
            agent_id: "high_skill_agent".into(),
            kind: "planner".into(),
            skills: vec!["analysis".into(), "market".into(), "eu".into()],
            supported_constraints: vec!["budget".into(), "data-locality".into()],
            trust_tier: TrustTier::T2Privileged,
            cost: CostModel { cost_per_call: 0.01, tokens_per_second: 150.0 },
            latency: LatencyStats { p50_ms: 40.0, p95_ms: 90.0 },
            success: SuccessStats { success_rate: 0.95, samples: 50 },
            provenance: None,
        });
    }
    if let Some(da) = ccos.get_delegating_arbiter() {
        use crate::ccos::arbiter_engine::ArbiterEngine;
        let intent = da.natural_language_to_intent("EU market analysis under budget", None).await.expect("intent");
        assert!(!intent.metadata.contains_key("delegation.selected_agent"), "delegation should be disabled by flag");
    } else { panic!("delegating arbiter missing"); }
}

#[tokio::test]
async fn test_delegation_governance_rejection_records_event() {
    std::env::set_var("CCOS_USE_DELEGATING_ARBITER", "1");
    std::env::set_var("CCOS_DELEGATING_MODEL", "stub-model");
    // Ensure delegation logic enabled
    std::env::remove_var("CCOS_DELEGATION_ENABLED");

    let ccos = CCOS::new().await.expect("init ccos");
    {
        // Register agent likely selected but should be vetoed by governance (EU goal + non_eu agent id)
        let reg_arc = ccos.get_agent_registry();
        let mut reg = reg_arc.write().unwrap();
        reg.register(AgentDescriptor {
            agent_id: "analysis_non_eu_agent".into(),
            kind: "planner".into(),
            skills: vec!["analysis".into(), "eu".into(), "market".into()],
            supported_constraints: vec!["budget".into(), "data-locality".into()],
            trust_tier: TrustTier::T1Trusted,
            cost: CostModel { cost_per_call: 0.02, tokens_per_second: 120.0 },
            latency: LatencyStats { p50_ms: 60.0, p95_ms: 140.0 },
            success: SuccessStats { success_rate: 0.9, samples: 30 },
            provenance: None,
        });
    }
    if let Some(da) = ccos.get_delegating_arbiter() {
        use crate::ccos::arbiter_engine::ArbiterEngine;
        let intent = da.natural_language_to_intent("Comprehensive EU market analysis with strict EU data locality", None).await.expect("intent");
        // Governance should reject; thus no selected_agent metadata
        assert!(!intent.metadata.contains_key("delegation.selected_agent"), "delegation should have been vetoed by governance");
        // Check causal chain for a 'delegation.rejected' event
        let chain = ccos.get_causal_chain();
        let chain_locked = chain.lock().unwrap();
        let found_rejected = chain_locked.get_all_actions().iter().any(|a| {
            if let Some(fn_name) = &a.function_name { fn_name == "delegation.rejected" } else { false }
        });
        assert!(found_rejected, "expected delegation.rejected event in causal chain");
    } else { panic!("delegating arbiter missing"); }
}

#[tokio::test]
async fn test_delegation_completed_event_emitted() {
    std::env::set_var("CCOS_USE_DELEGATING_ARBITER", "1");
    std::env::set_var("CCOS_DELEGATING_MODEL", "stub-model");
    // Lower threshold to encourage delegation
    std::env::set_var("CCOS_DELEGATION_THRESHOLD", "0.1");

    let ccos = CCOS::new().await.expect("init ccos");
    {
        let reg_arc = ccos.get_agent_registry();
        let mut reg = reg_arc.write().unwrap();
        reg.register(AgentDescriptor {
            agent_id: "high_perf_agent".into(),
            kind: "planner".into(),
            skills: vec!["delegated".into(), "task".into(), "small".into()],
            supported_constraints: vec!["budget".into()],
            trust_tier: TrustTier::T2Privileged,
            cost: CostModel { cost_per_call: 0.01, tokens_per_second: 200.0 },
            latency: LatencyStats { p50_ms: 30.0, p95_ms: 70.0 },
            success: SuccessStats { success_rate: 0.98, samples: 60 },
            provenance: None,
        });
    }

    // Security context for plan execution
    let context = RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: vec!["ccos.echo".to_string(), "ccos.math.add".to_string()].into_iter().collect(),
        ..RuntimeContext::pure()
    };

    let request = "please perform a small delegated task with budget awareness";
    let _ = ccos.process_request(request, &context).await.expect("process_request");

    // Inspect causal chain for approved then completed events
    let chain = ccos.get_causal_chain();
    let chain_locked = chain.lock().unwrap();
    let mut saw_approved = false;
    let mut saw_completed = false;
    for a in chain_locked.get_all_actions() {
        if let Some(fn_name) = &a.function_name {
            if fn_name == "delegation.approved" { saw_approved = true; }
            if fn_name == "delegation.completed" { saw_completed = true; }
        }
    }
    assert!(saw_approved, "expected delegation.approved event");
    assert!(saw_completed, "expected delegation.completed event");
}
