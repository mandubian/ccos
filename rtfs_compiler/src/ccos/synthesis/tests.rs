#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_with_enhanced_errors;
    use std::sync::Arc;

    // Reuse small helpers from schema_builder tests
    use crate::ccos::synthesis::InteractionTurn;

    fn make_turn(i: usize, prompt: &str, ans: Option<&str>) -> InteractionTurn {
        InteractionTurn {
            turn_index: i,
            prompt: prompt.to_string(),
            answer: ans.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn test_generated_artifacts_parse() {
        let convo = vec![
            make_turn(0, "Where would you like to go?", Some("Paris")),
            make_turn(1, "What dates will you travel?", None),
        ];

            let _ccos = Arc::new(crate::ccos::CCOS::new().await.unwrap());

            // Run synthesis with no marketplace snapshot (legacy path)
            let result = synthesize_capabilities(&convo);

        // Each generated artifact should be parseable by RTFS parser
        if let Some(col) = &result.collector {
            parse_with_enhanced_errors(col, None).expect("collector should parse");
        } else {
            panic!("collector missing");
        }

        if let Some(plan) = &result.planner {
            parse_with_enhanced_errors(plan, None).expect("planner should parse");
        } else {
            panic!("planner missing");
        }

        if let Some(stub) = &result.stub {
            parse_with_enhanced_errors(stub, None).expect("stub should parse");
        } else {
            panic!("stub missing");
        }
    }
}

    #[test]
    fn test_planner_v0_1_direct_match() {
        // Build a minimal schema-like InteractionTurn set (use schema_builder helpers)
        let convo = vec![
            make_turn(0, "Q1", Some("A1")),
            make_turn(1, "Q2", None),
        ];

        // Extract schema to get required keys
        let schema = crate::ccos::synthesis::schema_builder::extract_param_schema(&convo);

        // Build a fake capability manifest that includes the required context/keys metadata
        use crate::ccos::capability_marketplace::types::CapabilityManifest;

        let mut metadata = std::collections::HashMap::new();
        // All keys from schema
        let keys_csv = schema.params.keys().cloned().collect::<Vec<_>>().join(",");
        metadata.insert("context/keys".to_string(), keys_csv.clone());

        let manifest = CapabilityManifest {
            id: "synth.domain.direct.cap.v1".to_string(),
            name: "direct".to_string(),
            description: "test".to_string(),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Local(
                crate::ccos::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Err(crate::runtime::error::RuntimeError::Generic(
                            "noop".to_string(),
                        ))
                    }),
                },
            ),
            version: "1.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata,
        };

        let planner_rtfs = crate::ccos::synthesis::artifact_generator::generate_planner_generic_v0_1(
            &schema,
            &convo,
            "synth.domain",
            &[manifest.clone()],
        );

        // Should contain the manifest id
        assert!(planner_rtfs.contains(&manifest.id));
    }

    #[test]
    fn test_planner_v0_1_fallback_stub() {
        let convo = vec![make_turn(0, "Q1", Some("A1"))];
        let schema = crate::ccos::synthesis::schema_builder::extract_param_schema(&convo);

        // Empty marketplace -> fallback
        let planner_rtfs = crate::ccos::synthesis::artifact_generator::generate_planner_generic_v0_1(
            &schema,
            &convo,
            "synth.domain",
            &[],
        );

        assert!(
            planner_rtfs.contains("AUTO-GENERATED PLANNER (embedded synthesis plan)")
                && planner_rtfs.contains("synth.domain.generated.capability.v1"),
            "expected embedded synthesis plan with generated capability hints, got: {}",
            planner_rtfs
        );
    }
