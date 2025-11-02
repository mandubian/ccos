//! Transform capability needs into intents for recursive synthesis

use crate::discovery::need_extractor::CapabilityNeed;
use crate::types::{Intent, IntentStatus, GenerationContext};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Transforms a capability need into an intent for recursive synthesis
pub struct IntentTransformer;

impl IntentTransformer {
    /// Convert a capability need into an intent
    /// 
    /// The capability need represents a missing capability that needs to be synthesized.
    /// This method transforms it into an Intent that can then go through the normal
    /// refinement and decomposition process (clarifying questions, plan generation, etc.)
    pub fn need_to_intent(
        need: &CapabilityNeed, 
        parent_intent_id: Option<&str>,
        related_capabilities: &[String], // Optional list of related capability IDs for examples
        parent_goal_summary: Option<&str>, // Optional summary of parent goal for context
    ) -> Intent {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Generate a simple unique ID using timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let intent_id = format!("intent-{}-{}", 
            parent_intent_id.unwrap_or("root"),
            timestamp
        );
        
        // Create a natural language goal from the capability need
        // Explicitly instruct the LLM to identify the required external service/API
        let examples_section = if related_capabilities.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nRelated capabilities already in marketplace (for reference): {}",
                related_capabilities.join(", ")
            )
        };
        
        // Add parent goal context if available to give LLM better understanding
        let parent_context = if let Some(parent_summary) = parent_goal_summary {
            format!(
                "\n\nCONTEXT: This capability is needed to fulfill the parent goal: \"{}\". Your synthesized capability should contribute to achieving this goal.",
                parent_summary
            )
        } else {
            String::new()
        };
        
        let goal = format!(
            "Identify the specific external service, API, or capability needed to implement: {} (inputs: {}, outputs: {}). Then generate a plan that calls this service. IMPORTANT: Do NOT ask users questions - instead, declare the specific service capability ID you need (e.g., 'restaurant.api.search', 'hotel.booking.reserve', etc.) and use it in a (call :service.id {{...}}) form. If the service doesn't exist yet, declare what it should be called.{}{}",
            need.capability_class,
            need.required_inputs.join(", "),
            need.expected_outputs.join(", "),
            examples_section,
            parent_context
        );
        
        // Build constraints from required inputs
        let mut constraints = HashMap::new();
        constraints.insert(
            "must_declare_service".to_string(),
            Value::String("Plan must explicitly declare and call a specific service capability ID (e.g., restaurant.api.search), not ask user questions".to_string()),
        );
        for input in &need.required_inputs {
            constraints.insert(
                format!("must_accept_{}", input),
                Value::String(input.clone()),
            );
        }
        
        // Build preferences from expected outputs
        let mut preferences = HashMap::new();
        for output in &need.expected_outputs {
            preferences.insert(
                format!("must_produce_{}", output),
                Value::String(output.clone()),
            );
        }
        
        // Add the capability class as metadata
        let mut metadata = HashMap::new();
        metadata.insert("capability_class".to_string(), Value::String(need.capability_class.clone()));
        metadata.insert("synthesis_source".to_string(), Value::String("recursive_discovery".to_string()));
        if let Some(parent) = parent_intent_id {
            metadata.insert("parent_intent_id".to_string(), Value::String(parent.to_string()));
        }
        
        Intent {
            intent_id: intent_id.clone(),
            name: Some(format!("Synthesize {}", need.capability_class)),
            original_request: goal.clone(),
            goal,
            constraints,
            preferences,
            success_criteria: Some(Value::String(format!(
                "Capability {} is successfully implemented and registered",
                need.capability_class
            ))),
            status: IntentStatus::Active,
            created_at: now,
            updated_at: now,
            metadata,
        }
    }
    
    /// Create a generation context for the synthesized intent
    pub fn create_synthesis_context(rationale: &str) -> GenerationContext {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let mut input_context = HashMap::new();
        input_context.insert("rationale".to_string(), rationale.to_string());
        
        GenerationContext {
            arbiter_version: "recursive-synthesizer-1.0".to_string(),
            generation_timestamp: now,
            input_context,
            reasoning_trace: Some(format!("Recursive synthesis triggered: {}", rationale)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_need_to_intent() {
        let need = CapabilityNeed::new(
            "restaurant.reservation.book".to_string(),
            vec!["restaurant".to_string(), "date".to_string(), "party_size".to_string()],
            vec!["reservation_id".to_string()],
            "Need to book restaurant reservations".to_string(),
        );
        
        let intent = IntentTransformer::need_to_intent(&need, None, &[], None);
        
        assert_eq!(intent.name, Some("Synthesize restaurant.reservation.book".to_string()));
        assert!(intent.goal.contains("restaurant.reservation.book"));
        assert_eq!(intent.constraints.len(), 4); // must_declare_service + 3 inputs
        assert_eq!(intent.preferences.len(), 1);
        assert_eq!(intent.status, IntentStatus::Active);
    }
    
    #[test]
    fn test_need_to_intent_with_parent() {
        let need = CapabilityNeed::new(
            "test.capability".to_string(),
            vec!["input1".to_string()],
            vec!["output1".to_string()],
            "Test rationale".to_string(),
        );
        
        let intent = IntentTransformer::need_to_intent(&need, Some("parent-123"), &[], None);
        
        assert!(intent.intent_id.contains("parent-123"));
        assert_eq!(
            intent.metadata.get("parent_intent_id"),
            Some(&Value::String("parent-123".to_string()))
        );
    }
    
    #[test]
    fn test_need_to_intent_with_parent_summary() {
        let need = CapabilityNeed::new(
            "restaurant.search".to_string(),
            vec!["query".to_string(), "location".to_string()],
            vec!["results".to_string()],
            "Need to search restaurants".to_string(),
        );
        
        let intent = IntentTransformer::need_to_intent(
            &need, 
            Some("parent-123"), 
            &[], 
            Some("plan a trip to Paris")
        );
        
        // Should include parent goal in the generated intent
        assert!(intent.goal.contains("plan a trip to Paris"));
        assert!(intent.goal.contains("CONTEXT"));
    }
}

