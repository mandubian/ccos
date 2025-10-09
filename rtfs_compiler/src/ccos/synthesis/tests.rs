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

        let ccos = Arc::new(crate::ccos::CCOS::new().await.unwrap());

        // Run synthesis
        let result = synthesize_capabilities(&convo, &ccos);

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
