//! Grounded LLM decomposition strategy
//!
//! Uses embeddings to pre-filter relevant tools, then provides them to the LLM
//! for more accurate decomposition with real tool knowledge.

use std::sync::Arc;
use async_trait::async_trait;

use super::intent_first::LlmProvider;
use super::{DecompositionContext, DecompositionError, DecompositionResult, DecompositionStrategy};
use crate::planner::modular_planner::types::{
    ApiAction, DomainHint, IntentType, SubIntent, ToolSummary,
};

/// Compute cosine similarity between two embeddings
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| (*x as f64) * (*y as f64)).sum();
    let mag_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let mag_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    
    dot / (mag_a * mag_b)
}

/// Embedding service trait for semantic tool matching
#[async_trait(?Send)]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embedding for text
    async fn embed(&self, text: &str) -> Result<Vec<f32>, DecompositionError>;
}

/// Grounded LLM decomposition strategy.
/// 
/// Pre-filters available tools using embeddings, then provides the most
/// relevant ones to the LLM for decomposition. This grounds the LLM's
/// knowledge in real available capabilities.
pub struct GroundedLlmDecomposition {
    llm_provider: Arc<dyn LlmProvider>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Maximum number of tools to include in prompt
    max_tools_in_prompt: usize,
    /// Similarity threshold for tool inclusion
    similarity_threshold: f64,
}

impl GroundedLlmDecomposition {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            llm_provider,
            embedding_provider: None,
            max_tools_in_prompt: 10,
            similarity_threshold: 0.3,
        }
    }
    
    pub fn with_embedding_provider(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }
    
    pub fn with_max_tools(mut self, max: usize) -> Self {
        self.max_tools_in_prompt = max;
        self
    }
    
    /// Filter tools by relevance to goal using embeddings
    async fn filter_relevant_tools<'a>(
        &self,
        goal: &str,
        available_tools: &'a [ToolSummary],
    ) -> Result<Vec<&'a ToolSummary>, DecompositionError> {
        let embedding_provider = match &self.embedding_provider {
            Some(p) => p,
            None => {
                // No embedding provider, return all tools (up to max)
                return Ok(available_tools.iter().take(self.max_tools_in_prompt).collect());
            }
        };
        
        let goal_embedding = embedding_provider.embed(goal).await?;
        
        let mut scored_tools: Vec<(&'a ToolSummary, f64)> = Vec::new();
        
        for tool in available_tools {
            let tool_text = format!("{} {}", tool.name, tool.description);
            let tool_embedding = embedding_provider.embed(&tool_text).await?;
            let similarity = cosine_similarity(&goal_embedding, &tool_embedding);
            
            if similarity >= self.similarity_threshold {
                scored_tools.push((tool, similarity));
            }
        }
        
        // Sort by similarity descending
        scored_tools.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // Take top N
        Ok(scored_tools
            .into_iter()
            .take(self.max_tools_in_prompt)
            .map(|(tool, _)| tool)
            .collect())
    }
    
    fn build_grounded_prompt(&self, goal: &str, tools: &[&ToolSummary], context: &DecompositionContext) -> String {
        let tools_list = if tools.is_empty() {
            "No specific tools available - decompose into abstract steps.".to_string()
        } else {
            let mut list = String::from("AVAILABLE TOOLS (prefer these for api_call steps):\n");
            for tool in tools {
                list.push_str(&format!("- {}: {}\n", tool.name, tool.description));
            }
            list
        };
        
        let params_hint = if context.pre_extracted_params.is_empty() {
            String::new()
        } else {
            format!("\n\nAlready extracted parameters: {:?}", context.pre_extracted_params)
        };
        
        format!(r#"You are a goal decomposition expert. Break down the following goal into a MINIMAL number of steps.

{tools_list}

CRITICAL RULES:
1. MINIMIZE STEPS: Use the fewest steps possible to accomplish the goal.
2. PREFER TOOLS: If a tool above can accomplish part of the goal, use it! Include the tool name in the "tool" field.
3. FILTERING/PAGINATION ARE PARAMS: Most APIs support filter/sort/pagination as parameters. Do NOT create separate "filter" or "paginate" steps.
4. Use "data_transform" ONLY for client-side processing that no API tool can do.
5. Match tool names EXACTLY from the list above (e.g., "list_issues" not "github.list_issues").

INTENT TYPES:
- "user_input": Ask the user for information
- "api_call": External API operation - ALWAYS include "tool" field if a matching tool exists above
- "data_transform": ONLY for client-side processing (avoid if API can do it)
- "output": Display results

GOAL: "{goal}"
{params_hint}

Respond with ONLY a JSON object:
{{
  "steps": [
    {{
      "description": "Step description",
      "intent_type": "user_input|api_call|data_transform|output",
      "action": "list|get|create|update|delete|search",
      "tool": "EXACT_tool_name_from_list_above",
      "depends_on": [],
      "params": {{"filter": "user_value", "perPage": "user_value"}}
    }}
  ],
  "domain": "github|slack|filesystem|database|web|generic"
}}

Example for "filter issues in mandubian/ccos per filter asked to user and paginate":
{{
  "steps": [
    {{
      "description": "Ask user for filter and pagination preferences",
      "intent_type": "user_input",
      "action": null,
      "tool": null,
      "depends_on": [],
      "params": {{"prompt_topic": "filter criteria and page size"}}
    }},
    {{
      "description": "Search issues with user filter and pagination",
      "intent_type": "api_call",
      "action": "search",
      "tool": "search_issues",
      "depends_on": [0],
      "params": {{"owner": "mandubian", "repo": "ccos"}}
    }},
    {{
      "description": "Display results",
      "intent_type": "output",
      "action": "display",
      "tool": null,
      "depends_on": [1],
      "params": {{}}
    }}
  ],
  "domain": "github"
}}
"#, tools_list = tools_list, goal = goal, params_hint = params_hint)
    }
}

#[async_trait(?Send)]
impl DecompositionStrategy for GroundedLlmDecomposition {
    fn name(&self) -> &str {
        "grounded_llm"
    }
    
    fn can_handle(&self, _goal: &str) -> f64 {
        // Can handle anything with tools, with higher confidence than intent_first
        0.6
    }
    
    async fn decompose(
        &self,
        goal: &str,
        available_tools: Option<&[ToolSummary]>,
        context: &DecompositionContext,
    ) -> Result<DecompositionResult, DecompositionError> {
        if context.is_at_max_depth() {
            return Err(DecompositionError::TooComplex(
                "Maximum decomposition depth reached".to_string()
            ));
        }
        
        // Filter relevant tools
        let filtered_tools = if let Some(tools) = available_tools {
            self.filter_relevant_tools(goal, tools).await?
        } else {
            vec![]
        };
        
        let prompt = self.build_grounded_prompt(goal, &filtered_tools, context);
        let response = self.llm_provider.generate_text(&prompt).await?;
        
        // Parse response (reuse logic from intent_first)
        let parsed = parse_grounded_response(&response)?;
        let sub_intents = convert_grounded_to_sub_intents(parsed, goal)?;
        
        let confidence = if filtered_tools.is_empty() { 0.5 } else { 0.75 };
        
        Ok(DecompositionResult::atomic(sub_intents, "grounded_llm")
            .with_confidence(confidence)
            .with_reasoning(format!(
                "Grounded LLM decomposition with {} tools considered",
                filtered_tools.len()
            )))
    }
}

// ============================================================================
// Response Parsing (similar to intent_first but with tool field)
// ============================================================================

#[derive(Debug, serde::Deserialize)]
struct GroundedResponse {
    steps: Vec<GroundedStep>,
    domain: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GroundedStep {
    description: String,
    intent_type: String,
    action: Option<String>,
    tool: Option<String>,
    #[serde(default)]
    depends_on: Vec<usize>,
    #[serde(default)]
    params: std::collections::HashMap<String, serde_json::Value>,
}

fn parse_grounded_response(response: &str) -> Result<GroundedResponse, DecompositionError> {
    let json_str = extract_json(response);
    serde_json::from_str(json_str)
        .map_err(|e| DecompositionError::ParseError(format!("Failed to parse: {}. Response: {}", e, response)))
}

fn extract_json(response: &str) -> &str {
    if let Some(start) = response.find("```json") {
        let after = &response[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = response.find("```") {
        let after = &response[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            return &response[start..=end];
        }
    }
    response.trim()
}

fn convert_grounded_to_sub_intents(
    parsed: GroundedResponse,
    goal: &str,
) -> Result<Vec<SubIntent>, DecompositionError> {
    let domain = parsed.domain
        .as_ref()
        .and_then(|d| match d.to_lowercase().as_str() {
            "github" => Some(DomainHint::GitHub),
            "slack" => Some(DomainHint::Slack),
            "filesystem" => Some(DomainHint::FileSystem),
            "database" => Some(DomainHint::Database),
            "web" => Some(DomainHint::Web),
            _ => None,
        })
        .or_else(|| DomainHint::infer_from_text(goal));
    
    let mut sub_intents = Vec::new();
    
    for step in parsed.steps {
        let intent_type = match step.intent_type.to_lowercase().as_str() {
            "user_input" => {
                let topic = step.params.get("prompt_topic")
                    .and_then(|v| v.as_str())
                    .unwrap_or("input")
                    .to_string();
                IntentType::UserInput { prompt_topic: topic }
            }
            "api_call" => {
                let action = step.action
                    .as_ref()
                    .map(|a| ApiAction::from_str(a))
                    .unwrap_or(ApiAction::Other("unknown".to_string()));
                IntentType::ApiCall { action }
            }
            "data_transform" => {
                use crate::planner::modular_planner::types::TransformType;
                let transform = step.action
                    .as_ref()
                    .map(|a| match a.to_lowercase().as_str() {
                        "filter" => TransformType::Filter,
                        "sort" => TransformType::Sort,
                        "count" => TransformType::Count,
                        "format" => TransformType::Format,
                        _ => TransformType::Other(a.clone()),
                    })
                    .unwrap_or(TransformType::Other("unknown".to_string()));
                IntentType::DataTransform { transform }
            }
            "output" => {
                use crate::planner::modular_planner::types::OutputFormat;
                IntentType::Output { format: OutputFormat::Display }
            }
            _ => IntentType::Composite,
        };
        
        let mut sub_intent = SubIntent::new(step.description.clone(), intent_type)
            .with_dependencies(step.depends_on.clone());
        
        if let Some(ref d) = domain {
            sub_intent = sub_intent.with_domain(d.clone());
        }
        
        // Add tool as a hint in params if provided
        if let Some(tool) = step.tool {
            sub_intent.extracted_params.insert("_suggested_tool".to_string(), tool);
        }
        
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
        return Err(DecompositionError::ParseError("No steps returned".to_string()));
    }
    
    Ok(sub_intents)
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
    async fn test_grounded_decomposition_with_tools() {
        let mock_response = r#"
        {
          "steps": [
            {
              "description": "List issues from repository",
              "intent_type": "api_call",
              "action": "list",
              "tool": "list_issues",
              "depends_on": [],
              "params": {"owner": "mandubian", "repo": "ccos"}
            }
          ],
          "domain": "github"
        }
        "#;
        
        let provider = Arc::new(MockLlmProvider { response: mock_response.to_string() });
        let strategy = GroundedLlmDecomposition::new(provider);
        let context = DecompositionContext::new();
        
        let tools = vec![
            ToolSummary::new("list_issues", "List issues in a repository")
                .with_domain(DomainHint::GitHub),
        ];
        
        let result = strategy
            .decompose("list issues in mandubian/ccos", Some(&tools), &context)
            .await
            .expect("Should decompose");
        
        assert_eq!(result.sub_intents.len(), 1);
        assert_eq!(
            result.sub_intents[0].extracted_params.get("_suggested_tool"),
            Some(&"list_issues".to_string())
        );
    }
}
