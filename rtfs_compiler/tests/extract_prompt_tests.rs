use rtfs_compiler::ccos::types::{Action, ActionType, ExecutionResult};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::{MapKey, Keyword};
use std::collections::HashMap;

// The function under test is exported from the crate's examples_helpers module
use rtfs_compiler::examples_helpers::extract_question_prompt_from_action;

// Helper to construct an Action with a string argument
fn action_with_string_prompt(prompt: &str) -> Action {
    Action {
        action_id: "a1".to_string(),
        parent_action_id: None,
        plan_id: "test-plan".to_string(),
        intent_id: "intent-1".to_string(),
        action_type: ActionType::PlanPaused,
        function_name: None,
        arguments: Some(vec![Value::String("checkpoint-1".to_string()), Value::String(prompt.to_string())]),
        result: None,
        cost: None,
        duration_ms: None,
        timestamp: 0,
        metadata: HashMap::new(),
    }
}

// Helper to construct an Action with a map argument containing a prompt key
fn action_with_map_prompt(prompt: &str) -> Action {
    let mut map = HashMap::new();
    map.insert(MapKey::Keyword(Keyword("prompt".to_string())), Value::String(prompt.to_string()));
    Action {
        action_id: "a2".to_string(),
        parent_action_id: None,
        plan_id: "test-plan".to_string(),
        intent_id: "intent-1".to_string(),
        action_type: ActionType::PlanPaused,
        function_name: None,
        arguments: Some(vec![Value::String("checkpoint-2".to_string()), Value::Map(map)]),
        result: None,
        cost: None,
        duration_ms: None,
        timestamp: 0,
        metadata: HashMap::new(),
    }
}

#[test]
fn test_extract_prompt_from_string_arg() {
    let a = action_with_string_prompt("What is your destination?");
    let p = extract_question_prompt_from_action(&a);
    assert_eq!(p.unwrap(), "What is your destination?");
}

#[test]
fn test_extract_prompt_from_map_arg() {
    let a = action_with_map_prompt("Please provide travel dates");
    let p = extract_question_prompt_from_action(&a);
    assert_eq!(p.unwrap(), "Please provide travel dates");
}
