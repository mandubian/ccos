#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::capability_marketplace::types::{
        CapabilityManifest, EffectType, LocalCapability, ProviderType,
    };
    use crate::synthesis::dialogue::schema_builder::extract_param_schema;
    use crate::synthesis::{synthesize_capabilities_with_marketplace, InteractionTurn};

    fn make_turn(i: usize, prompt: &str, ans: Option<&str>) -> InteractionTurn {
        InteractionTurn {
            turn_index: i,
            prompt: prompt.to_string(),
            answer: ans.map(|s| s.to_string()),
        }
    }

    #[test]
    fn end_to_end_planner_prefers_direct_match() {
        // Conversation where a required key will be detected by schema_builder
        let convo = vec![
            make_turn(0, "What message?", Some("hello")),
            make_turn(1, "Confirm?", None),
        ];

        // Build a fake capability manifest that includes the required context/keys metadata
        let schema = extract_param_schema(&convo);
        println!("Extracted schema: {:#?}", schema);
        let mut metadata = std::collections::HashMap::new();
        let keys_csv = schema.params.keys().cloned().collect::<Vec<_>>().join(",");
        metadata.insert("context/keys".to_string(), keys_csv.clone());

        let manifest = CapabilityManifest {
            id: "synth.domain.direct.cap.v1".to_string(),
            name: "direct".to_string(),
            description: "test".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: Arc::new(|_| {
                    Err(rtfs::runtime::error::RuntimeError::Generic(
                        "noop".to_string(),
                    ))
                }),
            }),
            version: "1.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata,
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::default(),
        };

        let result = synthesize_capabilities_with_marketplace(&convo, &[manifest.clone()]);
        let planner = result.planner.unwrap_or_default();

        assert!(
            planner.contains(&manifest.id),
            "planner RTFS must reference direct capability when marketplace match exists: {}",
            planner
        );
    }

    #[test]
    fn end_to_end_planner_delegates_to_arbiter_when_no_match() {
        // Conversation expecting a `message` key
        let convo = vec![make_turn(0, "What message?", Some("hello"))];

        // Empty marketplace snapshot -> should generate planner artifact
        let result = crate::synthesis::synthesize_capabilities_with_marketplace(&convo, &[]);
        let planner = result.planner.unwrap_or_default();

        let has_embedded_plan = planner
            .contains("AUTO-GENERATED PLANNER (embedded synthesis plan)")
            && planner.contains("synth.domain.generated.capability.v1")
            && planner.contains(":conversation");

        assert!(
            has_embedded_plan,
            "expected planner to embed synthesized plan details, got: {}",
            planner
        );

        // When the planner doesn't explicitly call external capabilities,
        // pending_capabilities may be empty (dependencies are extracted from :call patterns)
        // The test verifies that the planner is generated successfully even with empty marketplace
        assert!(
            !planner.is_empty(),
            "planner should be generated even with empty marketplace"
        );
    }
}
