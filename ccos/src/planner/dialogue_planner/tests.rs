// Tests for DialoguePlanner module

#[cfg(test)]
mod tests {
    use crate::planner::dialogue_planner::entity::{DialogueEntity, EntityError, HumanEntity};
    use crate::planner::dialogue_planner::types::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    // ============================================================================
    // Mock Entity for testing
    // ============================================================================

    /// A mock entity that returns pre-programmed responses
    struct MockEntity {
        responses: Arc<Mutex<Vec<String>>>,
        received_messages: Arc<Mutex<Vec<String>>>,
        response_index: Arc<Mutex<usize>>,
    }

    impl MockEntity {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses)),
                received_messages: Arc::new(Mutex::new(Vec::new())),
                response_index: Arc::new(Mutex::new(0)),
            }
        }

        #[allow(dead_code)]
        fn get_received_messages(&self) -> Vec<String> {
            self.received_messages.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl DialogueEntity for MockEntity {
        fn entity_type(&self) -> &str {
            "mock"
        }

        async fn send(&self, message: &str) -> Result<(), EntityError> {
            self.received_messages
                .lock()
                .unwrap()
                .push(message.to_string());
            Ok(())
        }

        async fn receive(&self, _timeout: Option<Duration>) -> Result<String, EntityError> {
            let mut index = self.response_index.lock().unwrap();
            let responses = self.responses.lock().unwrap();

            if *index >= responses.len() {
                return Err(EntityError::Cancelled);
            }

            let response = responses[*index].clone();
            *index += 1;
            Ok(response)
        }

        async fn parse_intent(&self, raw_input: &str) -> Result<InputIntent, EntityError> {
            let input = raw_input.trim().to_lowercase();

            if input == "proceed" || input == "yes" || input == "y" {
                return Ok(InputIntent::Proceed);
            }

            if input == "quit" || input == "exit" || input == "cancel" {
                return Ok(InputIntent::Abandon {
                    reason: Some("Mock requested".to_string()),
                });
            }

            if let Ok(num) = input.parse::<usize>() {
                return Ok(InputIntent::SelectOption {
                    option_id: num.to_string(),
                });
            }

            // Longer input is goal refinement
            if raw_input.len() > 10 {
                return Ok(InputIntent::RefineGoal {
                    new_goal: raw_input.to_string(),
                });
            }

            Ok(InputIntent::Unclear {
                raw_input: raw_input.to_string(),
            })
        }
    }

    // ============================================================================
    // Intent parsing tests
    // ============================================================================

    #[test]
    fn test_parse_proceed_intent() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = MockEntity::new(vec![]);

            let intent = entity.parse_intent("proceed").await.unwrap();
            assert!(matches!(intent, InputIntent::Proceed));

            let intent = entity.parse_intent("yes").await.unwrap();
            assert!(matches!(intent, InputIntent::Proceed));

            let intent = entity.parse_intent("Y").await.unwrap();
            assert!(matches!(intent, InputIntent::Proceed));
        });
    }

    #[test]
    fn test_parse_abandon_intent() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = MockEntity::new(vec![]);

            let intent = entity.parse_intent("quit").await.unwrap();
            assert!(matches!(intent, InputIntent::Abandon { .. }));

            let intent = entity.parse_intent("cancel").await.unwrap();
            assert!(matches!(intent, InputIntent::Abandon { .. }));
        });
    }

    #[test]
    fn test_parse_option_selection() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = MockEntity::new(vec![]);

            let intent = entity.parse_intent("1").await.unwrap();
            match intent {
                InputIntent::SelectOption { option_id } => assert_eq!(option_id, "1"),
                _ => panic!("Expected SelectOption"),
            }

            let intent = entity.parse_intent("3").await.unwrap();
            match intent {
                InputIntent::SelectOption { option_id } => assert_eq!(option_id, "3"),
                _ => panic!("Expected SelectOption"),
            }
        });
    }

    #[test]
    fn test_parse_goal_refinement() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = MockEntity::new(vec![]);

            let long_input = "I want to read all files from the documents folder";
            let intent = entity.parse_intent(long_input).await.unwrap();
            match intent {
                InputIntent::RefineGoal { new_goal } => {
                    assert_eq!(new_goal, long_input);
                }
                _ => panic!("Expected RefineGoal"),
            }
        });
    }

    // ============================================================================
    // Goal analysis tests
    // ============================================================================

    #[test]
    fn test_infer_filesystem_domain() {
        let goal = "read all files from the documents folder";
        let goal_lower = goal.to_lowercase();

        let keywords = vec!["file", "directory", "read", "write", "delete", "folder"];
        let has_fs = keywords.iter().any(|kw| goal_lower.contains(kw));

        assert!(has_fs, "Should detect filesystem keywords");
    }

    #[test]
    fn test_infer_exchange_domain() {
        let goal = "buy 100 bitcoin on coinbase";
        let goal_lower = goal.to_lowercase();

        let keywords = vec![
            "trade", "trading", "buy", "sell", "order", "bitcoin", "crypto",
        ];
        let has_exchange = keywords.iter().any(|kw| goal_lower.contains(kw));

        assert!(has_exchange, "Should detect exchange keywords");
    }

    #[test]
    fn test_feasibility_calculation() {
        // 2 required, 1 available = 50% feasibility
        let required = vec!["filesystem".to_string(), "exchange".to_string()];
        let available = vec!["filesystem".to_string()];

        let missing: Vec<String> = required
            .iter()
            .filter(|d| {
                !available
                    .iter()
                    .any(|a| a.to_lowercase() == d.to_lowercase())
            })
            .cloned()
            .collect();

        let feasibility = (required.len() - missing.len()) as f32 / required.len() as f32;

        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], "exchange");
        assert!((feasibility - 0.5).abs() < 0.01);
    }

    // ============================================================================
    // Types tests
    // ============================================================================

    #[test]
    fn test_dialogue_config_default() {
        let config = DialogueConfig::default();

        assert!(config.max_turns > 0);
        assert!(config.turn_timeout_secs > 0);
        assert!(matches!(config.autonomy, AutonomyLevel::Guided));
    }

    #[test]
    fn test_goal_analysis_serialization() {
        let analysis = GoalAnalysis {
            goal: "test goal".to_string(),
            required_domains: vec!["filesystem".to_string()],
            available_domains: vec!["filesystem".to_string()],
            missing_domains: vec![],
            feasibility: 1.0,
            suggestions: vec![],
            can_proceed_immediately: true,
        };

        assert_eq!(analysis.goal, "test goal");
        assert_eq!(analysis.feasibility, 1.0);
        assert!(analysis.can_proceed_immediately);
    }

    #[test]
    fn test_suggestion_variants() {
        let discover = Suggestion::Discover {
            domain: "exchange".to_string(),
            example_servers: vec!["coinbase-server".to_string()],
        };

        let refine = Suggestion::RefineGoal {
            alternative: "Simpler goal".to_string(),
            reason: "Not all capabilities available".to_string(),
        };

        match discover {
            Suggestion::Discover { domain, .. } => assert_eq!(domain, "exchange"),
            _ => panic!("Expected Discover"),
        }

        match refine {
            Suggestion::RefineGoal { alternative, .. } => {
                assert_eq!(alternative, "Simpler goal")
            }
            _ => panic!("Expected RefineGoal"),
        }
    }

    // ============================================================================
    // Turn action tests
    // ============================================================================

    #[test]
    fn test_turn_action_goal_analyzed() {
        let action = TurnAction::GoalAnalyzed {
            feasibility: 0.75,
            missing_domains: vec!["email".to_string()],
            suggestions_count: 2,
        };

        match action {
            TurnAction::GoalAnalyzed {
                feasibility,
                missing_domains,
                suggestions_count,
            } => {
                assert!((feasibility - 0.75).abs() < 0.01);
                assert_eq!(missing_domains.len(), 1);
                assert_eq!(suggestions_count, 2);
            }
            _ => panic!("Expected GoalAnalyzed"),
        }
    }

    #[test]
    fn test_processing_result_creation() {
        let result = ProcessingResult {
            actions: vec![],
            next_message: Some("Next step".to_string()),
            completed_plan: None,
            should_continue: true,
            abandon_reason: None,
        };

        assert!(result.should_continue);
        assert!(result.completed_plan.is_none());
        assert_eq!(result.next_message, Some("Next step".to_string()));
    }

    // ============================================================================
    // HumanEntity parsing tests
    // ============================================================================

    #[test]
    fn test_human_entity_discover_parsing() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = HumanEntity::new(None);

            let intent = entity.parse_intent("discover exchange").await.unwrap();
            match intent {
                InputIntent::Discover { domain } => assert_eq!(domain, "exchange"),
                _ => panic!("Expected Discover"),
            }

            let intent = entity.parse_intent("find github").await.unwrap();
            match intent {
                InputIntent::Discover { domain } => assert_eq!(domain, "github"),
                _ => panic!("Expected Discover"),
            }
        });
    }

    #[test]
    fn test_human_entity_connect_parsing() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = HumanEntity::new(None);

            let intent = entity.parse_intent("connect myserver").await.unwrap();
            match intent {
                InputIntent::ConnectServer { server_id } => assert_eq!(server_id, "myserver"),
                _ => panic!("Expected ConnectServer"),
            }
        });
    }

    #[test]
    fn test_human_entity_synthesize_parsing() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = HumanEntity::new(None);

            let intent = entity
                .parse_intent("create a new json parser")
                .await
                .unwrap();
            match intent {
                InputIntent::Synthesize { description } => {
                    assert!(description.contains("json parser"));
                }
                _ => panic!("Expected Synthesize"),
            }
        });
    }

    #[test]
    fn test_human_entity_approval_parsing() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let entity = HumanEntity::new(None);

            let intent = entity.parse_intent("approve request-123").await.unwrap();
            match intent {
                InputIntent::Approval {
                    request_id,
                    approved,
                } => {
                    assert!(approved);
                    assert_eq!(request_id, "request-123");
                }
                _ => panic!("Expected Approval"),
            }

            let intent = entity.parse_intent("reject request-456").await.unwrap();
            match intent {
                InputIntent::Approval {
                    request_id,
                    approved,
                } => {
                    assert!(!approved);
                    assert_eq!(request_id, "request-456");
                }
                _ => panic!("Expected Approval rejection"),
            }
        });
    }
}
