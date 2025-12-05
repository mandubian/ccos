//! Intent-first LLM decomposition strategy
//!
//! Uses an LLM to decompose goals into abstract intents WITHOUT providing
//! tool hints. The LLM focuses on WHAT needs to be done, not HOW.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{DecompositionContext, DecompositionError, DecompositionResult, DecompositionStrategy};
use crate::planner::modular_planner::types::{
    ApiAction, DomainHint, IntentType, OutputFormat, SubIntent, ToolSummary, TransformType,
};

/// LLM provider trait for generating decompositions
#[async_trait(?Send)]
pub trait LlmProvider: Send + Sync {
    async fn generate_text(&self, prompt: &str) -> Result<String, DecompositionError>;
}

/// Intent-first decomposition strategy.
///
/// Asks the LLM to decompose a goal into abstract intents, focusing on
/// semantics rather than specific tools. This produces cleaner decompositions
/// that are easier to resolve to actual capabilities.
pub struct IntentFirstDecomposition {
    llm_provider: Arc<dyn LlmProvider>,
}

impl IntentFirstDecomposition {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }
}

#[async_trait(?Send)]
impl DecompositionStrategy for IntentFirstDecomposition {
    fn name(&self) -> &str {
        "intent_first"
    }

    fn can_handle(&self, _goal: &str) -> f64 {
        // LLM can attempt any goal, but with moderate confidence
        // (patterns should be preferred when they match)
        0.5
    }

    async fn decompose(
        &self,
        goal: &str,
        _available_tools: Option<&[ToolSummary]>,
        context: &DecompositionContext,
    ) -> Result<DecompositionResult, DecompositionError> {
        if context.is_at_max_depth() {
            return Err(DecompositionError::TooComplex(
                "Maximum decomposition depth reached".to_string(),
            ));
        }

        let prompt = build_decomposition_prompt(goal, context);

        // DEBUG: Print prompt
        println!("\n🤖 LLM Prompt (IntentFirst):\n--------------------------------------------------\n{}\n--------------------------------------------------", prompt);

        let response = self.llm_provider.generate_text(&prompt).await?;

        // DEBUG: Print response
        println!("\n🤖 LLM Response (IntentFirst):\n--------------------------------------------------\n{}\n--------------------------------------------------", response);

        let parsed = parse_llm_response(&response)?;

        // Convert parsed response to SubIntents
        let sub_intents = convert_to_sub_intents(parsed, goal)?;

        Ok(DecompositionResult::atomic(sub_intents, "intent_first")
            .with_confidence(0.7)
            .with_reasoning(format!("LLM decomposition of: {}", goal)))
    }
}

// ============================================================================
// LLM Prompt Construction
// ============================================================================

fn build_decomposition_prompt(goal: &str, context: &DecompositionContext) -> String {
    let params_hint = if context.pre_extracted_params.is_empty() {
        String::new()
    } else {
        let mut lines = Vec::new();
        for (k, v) in context.pre_extracted_params.iter() {
            lines.push(format!("{}: {}", k, v));
        }
        let joined = lines.join("\n");
        format!(
            r#"

Grounded data (prefer using this instead of asking user):
{}
RULE: If grounded data covers what you need, use data_transform/output. Only ask user if required params are truly missing or ambiguous. Prefer the most recent result for outputs when relevant."#,
            joined
        )
    };

    format!(
        r#"You are a goal decomposition expert. Break down the following goal into a sequence of simple, atomic steps.

IMPORTANT RULES:
1. Focus on WHAT needs to be done, not specific tools or APIs.
2. Each step should have ONE clear purpose.
3. Use semantic intent types, not tool names.
4. Identify dependencies between steps (what step needs output from another).
5. Extract any parameters mentioned in the goal.

INTENT TYPES (use these exactly):
- "user_input": Ask the user for information
- "api_call": Fetch or modify external data (list, get, create, update, delete, search)
- "data_transform": Process data (filter, sort, count, format, extract)
- "output": Display results to user

GOAL: "{goal}"
{params_hint}

Respond with ONLY a JSON object in this exact format:
{{
  "steps": [
    {{
      "description": "Clear description of what this step does",
      "intent_type": "user_input|api_call|data_transform|output",
      "action": "for api_call: list|get|create|update|delete|search. for data_transform: filter|sort|count|format|extract",
      "depends_on": [0],  // indices of steps this depends on (empty array if none)
      "params": {{"key": "value"}}  // any parameters relevant to this step
    }}
  ],
  "domain": "github|slack|filesystem|database|web|generic"  // inferred domain
}}

Example for "list issues in mandubian/ccos but ask me for page size":
{{
  "steps": [
    {{
      "description": "Ask user for desired page size",
      "intent_type": "user_input",
      "action": null,
      "depends_on": [],
      "params": {{"prompt_topic": "page size"}}
    }},
    {{
      "description": "List issues from repository",
      "intent_type": "api_call",
      "action": "list",
      "depends_on": [0],
      "params": {{"owner": "mandubian", "repo": "ccos", "resource": "issues"}}
    }}
  ],
  "domain": "github"
}}
"#,
        goal = goal,
        params_hint = params_hint
    )
}

// ============================================================================
// Response Parsing
// ============================================================================

#[derive(Debug, Deserialize)]
struct LlmResponse {
    steps: Vec<LlmStep>,
    domain: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlmStep {
    description: String,
    intent_type: String,
    action: Option<String>,
    #[serde(default)]
    depends_on: Vec<usize>,
    #[serde(default)]
    params: std::collections::HashMap<String, serde_json::Value>,
}

fn parse_llm_response(response: &str) -> Result<LlmResponse, DecompositionError> {
    // Extract JSON from response (handle markdown code blocks)
    let json_str = extract_json(response);

    serde_json::from_str(json_str).map_err(|e| {
        DecompositionError::ParseError(format!(
            "Failed to parse LLM response: {}. Response was: {}",
            e, response
        ))
    })
}

fn extract_json(response: &str) -> &str {
    // Try to find JSON in markdown code block
    if let Some(start) = response.find("```json") {
        let after_marker = &response[start + 7..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim();
        }
    }

    // Try plain code block
    if let Some(start) = response.find("```") {
        let after_marker = &response[start + 3..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim();
        }
    }

    // Try to find raw JSON object
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            return &response[start..=end];
        }
    }

    response.trim()
}

fn convert_to_sub_intents(
    parsed: LlmResponse,
    goal: &str,
) -> Result<Vec<SubIntent>, DecompositionError> {
    let domain = parsed
        .domain
        .as_ref()
        .and_then(|d| match d.to_lowercase().as_str() {
            "github" => Some(DomainHint::GitHub),
            "slack" => Some(DomainHint::Slack),
            "filesystem" | "fs" => Some(DomainHint::FileSystem),
            "database" | "db" => Some(DomainHint::Database),
            "web" | "http" => Some(DomainHint::Web),
            "email" => Some(DomainHint::Email),
            "calendar" => Some(DomainHint::Calendar),
            _ => None,
        })
        .or_else(|| DomainHint::infer_from_text(goal));

    let mut sub_intents = Vec::new();

    for step in parsed.steps {
        let intent_type = parse_intent_type(&step)?;

        let mut sub_intent = SubIntent::new(step.description.clone(), intent_type)
            .with_dependencies(step.depends_on.clone());

        if let Some(ref d) = domain {
            sub_intent = sub_intent.with_domain(d.clone());
        }

        // Convert params
        for (key, value) in step.params {
            let str_value = match value {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            sub_intent.extracted_params.insert(key, str_value);
        }

        sub_intents.push(sub_intent);
    }

    if sub_intents.is_empty() {
        return Err(DecompositionError::ParseError(
            "LLM returned no steps".to_string(),
        ));
    }

    Ok(sub_intents)
}

fn parse_intent_type(step: &LlmStep) -> Result<IntentType, DecompositionError> {
    match step.intent_type.to_lowercase().as_str() {
        "user_input" => {
            let prompt_topic = step
                .params
                .get("prompt_topic")
                .and_then(|v| v.as_str())
                .unwrap_or("input")
                .to_string();
            Ok(IntentType::UserInput { prompt_topic })
        }

        "api_call" => {
            let action = step
                .action
                .as_ref()
                .map(|a| ApiAction::from_str(a))
                .unwrap_or(ApiAction::Other("unknown".to_string()));
            Ok(IntentType::ApiCall { action })
        }

        "data_transform" => {
            let transform = step
                .action
                .as_ref()
                .map(|a| match a.to_lowercase().as_str() {
                    "filter" => TransformType::Filter,
                    "sort" => TransformType::Sort,
                    "count" => TransformType::Count,
                    "format" => TransformType::Format,
                    "extract" => TransformType::Extract,
                    "group" | "groupby" => TransformType::GroupBy,
                    "aggregate" => TransformType::Aggregate,
                    "parse" => TransformType::Parse,
                    "validate" => TransformType::Validate,
                    other => TransformType::Other(other.to_string()),
                })
                .unwrap_or(TransformType::Other("unknown".to_string()));
            Ok(IntentType::DataTransform { transform })
        }

        "output" => {
            let format = step
                .action
                .as_ref()
                .map(|a| match a.to_lowercase().as_str() {
                    "display" | "show" => OutputFormat::Display,
                    "print" => OutputFormat::Print,
                    "json" => OutputFormat::Json,
                    "table" => OutputFormat::Table,
                    "summary" => OutputFormat::Summary,
                    other => OutputFormat::Other(other.to_string()),
                })
                .unwrap_or(OutputFormat::Display);
            Ok(IntentType::Output { format })
        }

        "composite" => Ok(IntentType::Composite),

        other => Err(DecompositionError::ParseError(format!(
            "Unknown intent type: {}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockLlmProvider {
        response: String,
    }

    #[async_trait(?Send)]
    impl LlmProvider for MockLlmProvider {
        async fn generate_text(&self, _prompt: &str) -> Result<String, DecompositionError> {
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn test_intent_first_decomposition() {
        let mock_response = r#"
        {
          "steps": [
            {
              "description": "Ask user for page size",
              "intent_type": "user_input",
              "action": null,
              "depends_on": [],
              "params": {"prompt_topic": "page size"}
            },
            {
              "description": "List issues from repository",
              "intent_type": "api_call",
              "action": "list",
              "depends_on": [0],
              "params": {"owner": "mandubian", "repo": "ccos"}
            }
          ],
          "domain": "github"
        }
        "#;

        let provider = Arc::new(MockLlmProvider {
            response: mock_response.to_string(),
        });
        let strategy = IntentFirstDecomposition::new(provider);
        let context = DecompositionContext::new();

        let result = strategy
            .decompose(
                "list issues in mandubian/ccos but ask me for page size",
                None,
                &context,
            )
            .await
            .expect("Should decompose");

        assert_eq!(result.sub_intents.len(), 2);
        assert!(matches!(
            result.sub_intents[0].intent_type,
            IntentType::UserInput { .. }
        ));
        assert!(matches!(
            result.sub_intents[1].intent_type,
            IntentType::ApiCall {
                action: ApiAction::List
            }
        ));
        assert_eq!(result.sub_intents[1].dependencies, vec![0]);
        assert_eq!(result.sub_intents[1].domain_hint, Some(DomainHint::GitHub));
    }

    #[test]
    fn test_extract_json() {
        let with_markdown = "Here's the result:\n```json\n{\"test\": 1}\n```";
        assert_eq!(extract_json(with_markdown), "{\"test\": 1}");

        let plain_json = "{\"test\": 2}";
        assert_eq!(extract_json(plain_json), "{\"test\": 2}");

        let with_text = "Some text before {\"test\": 3} and after";
        assert_eq!(extract_json(with_text), "{\"test\": 3}");
    }
}
