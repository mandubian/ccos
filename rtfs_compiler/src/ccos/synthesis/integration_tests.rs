#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::ccos::capability_marketplace::types::{
        CapabilityManifest, LocalCapability, ProviderType,
    };
    use crate::ccos::synthesis::schema_builder::extract_param_schema;
    use crate::ccos::synthesis::{synthesize_capabilities_with_marketplace, InteractionTurn};

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
                    Err(crate::runtime::error::RuntimeError::Generic(
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

        // Empty marketplace snapshot -> should produce pending capabilities
        let result = crate::ccos::synthesis::synthesize_capabilities_with_marketplace(&convo, &[]);
        let planner = result.planner.unwrap_or_default();

        let has_embedded_plan = planner.contains("AUTO-GENERATED PLANNER (embedded synthesis plan)")
            && planner.contains("synth.domain.generated.capability.v1")
            && planner.contains(":conversation");

        assert!(
            has_embedded_plan,
            "expected planner to embed synthesized plan details, got: {}",
            planner
        );

        assert!(
            !result.pending_capabilities.is_empty(),
            "should have pending capabilities when marketplace is empty, got: {:?}",
            result.pending_capabilities
        );
    }
}
