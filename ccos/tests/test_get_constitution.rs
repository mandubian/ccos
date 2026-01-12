use ccos::governance_kernel::{GovernanceKernel, RuleAction};
use ccos::intent_graph::IntentGraph;
use ccos::orchestrator::Orchestrator;
use std::sync::{Arc, Mutex};

#[test]
fn test_get_constitution_serialization() {
    // Setup minimal dependencies
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new()));
    // Mock orchestrator (simplified, or use real one if easy)
    // Orchestrator::new requires a lot of deps.
    // Let's see if we can instantiate GovernanceKernel without full Orchestrator.
    // GovernanceKernel::new takes Arc<Orchestrator>.
    // Maybe we just test the Constitution struct serialization directly?
    // But `get_rules` is on GovernanceKernel.

    // Actually, checking `GovernanceKernel::new` in `governance_kernel.rs`:
    // pub fn new(orchestrator: Arc<Orchestrator>, intent_graph: Arc<Mutex<IntentGraph>>) -> Self

    // Instantiating Orchestrator is heavy.
    // However, I just made `Constitution` public and its fields public.
    // So I can test `Constitution::default()` and its serialization directly.
    // The `ccos_get_constitution` tool just wraps `get_rules()` which returns `&Constitution`.

    use ccos::governance_kernel::Constitution;

    let constitution = Constitution::default();
    let rules = &constitution.rules;

    // Verify default rules exist
    assert!(rules.iter().any(|r| r.id == "cli-agent-restrictions"));
    assert!(rules.iter().any(|r| r.id == "no-global-thermonuclear-war"));

    // Test Serialization
    let json = serde_json::to_value(&constitution).expect("Failed to serialize constitution");

    // Verify JSON structure
    assert!(json.get("rules").is_some());
    let rules_json = json
        .get("rules")
        .unwrap()
        .as_array()
        .expect("rules should be an array");

    // Find specific rule in JSON
    let nuke_rule = rules_json
        .iter()
        .find(|v| v.get("id").and_then(|i| i.as_str()) == Some("no-global-thermonuclear-war"))
        .expect("Should find nuke rule in JSON");

    assert_eq!(
        nuke_rule.get("match_pattern").and_then(|s| s.as_str()),
        Some("*launch-nukes*")
    );

    // Check action serialization
    let action = nuke_rule.get("action").expect("Rule should have action");
    // Action is an Enum. Serde defaults to {"Variant": content} or "Variant" for unit variants.
    // RuleAction::Deny(String) -> {"Deny": "reason"}
    assert!(action.get("Deny").is_some());
}
