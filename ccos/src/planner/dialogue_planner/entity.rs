// Entity abstraction: Humans, LLMs, or other agents

use async_trait::async_trait;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use super::types::InputIntent;

/// Trait for dialogue entities (humans, LLMs, agents)
#[async_trait]
pub trait DialogueEntity: Send + Sync {
    /// Get entity type name
    fn entity_type(&self) -> &str;

    /// Send a message to the entity
    async fn send(&self, message: &str) -> Result<(), EntityError>;

    /// Receive response from the entity (with optional timeout)
    async fn receive(&self, timeout: Option<Duration>) -> Result<String, EntityError>;

    /// Send and receive in one call
    async fn exchange(
        &self,
        message: &str,
        timeout: Option<Duration>,
    ) -> Result<String, EntityError> {
        self.send(message).await?;
        self.receive(timeout).await
    }

    /// Parse the raw response into an InputIntent
    async fn parse_intent(&self, raw_input: &str) -> Result<InputIntent, EntityError>;
}

/// Errors from entity operations
#[derive(Debug, thiserror::Error)]
pub enum EntityError {
    #[error("Entity timed out waiting for response")]
    Timeout,
    #[error("Entity connection lost")]
    Disconnected,
    #[error("I/O error: {0}")]
    IoError(String),
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Entity cancelled the dialogue")]
    Cancelled,
}

impl From<io::Error> for EntityError {
    fn from(err: io::Error) -> Self {
        EntityError::IoError(err.to_string())
    }
}

// ============================================================================
// Human Entity (CLI-based)
// ============================================================================

/// Human entity interacting via CLI (stdin/stdout)
pub struct HumanEntity {
    name: Option<String>,
}

impl HumanEntity {
    pub fn new(name: Option<String>) -> Self {
        Self { name }
    }

    pub fn anonymous() -> Self {
        Self { name: None }
    }
}

#[async_trait]
impl DialogueEntity for HumanEntity {
    fn entity_type(&self) -> &str {
        "human"
    }

    async fn send(&self, message: &str) -> Result<(), EntityError> {
        // Format message nicely for human
        println!();
        for line in message.lines() {
            println!("  💬 {}", line);
        }
        println!();
        Ok(())
    }

    async fn receive(&self, _timeout: Option<Duration>) -> Result<String, EntityError> {
        print!("  You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let input = input.trim().to_string();

        if input.to_lowercase() == "quit" || input.to_lowercase() == "exit" {
            return Err(EntityError::Cancelled);
        }

        Ok(input)
    }

    async fn parse_intent(&self, raw_input: &str) -> Result<InputIntent, EntityError> {
        let input = raw_input.trim().to_lowercase();

        // Simple keyword-based intent parsing for humans
        // Future: Could use LLM for more sophisticated parsing

        if input.is_empty() {
            return Ok(InputIntent::Unclear {
                raw_input: raw_input.to_string(),
            });
        }

        // Check for abandonment
        if input == "quit" || input == "exit" || input == "cancel" || input == "abort" {
            return Ok(InputIntent::Abandon {
                reason: Some("User requested".to_string()),
            });
        }

        // Check for proceed/continue
        if input == "proceed"
            || input == "continue"
            || input == "yes"
            || input == "ok"
            || input == "y"
        {
            return Ok(InputIntent::Proceed);
        }

        // Check for discovery keywords
        if input.starts_with("discover") || input.starts_with("find") || input.starts_with("search")
        {
            let domain = input
                .replace("discover", "")
                .replace("find", "")
                .replace("search", "")
                .trim()
                .to_string();
            if !domain.is_empty() {
                return Ok(InputIntent::Discover { domain });
            }
        }

        // Check for connection keywords
        if input.starts_with("connect") || input.starts_with("use") {
            let server = input
                .replace("connect", "")
                .replace("use", "")
                .trim()
                .to_string();
            if !server.is_empty() {
                return Ok(InputIntent::ConnectServer { server_id: server });
            }
        }

        // Check for synthesis keywords
        if input.starts_with("create")
            || input.starts_with("synthesize")
            || input.starts_with("make")
        {
            let description = input
                .replace("create", "")
                .replace("synthesize", "")
                .replace("make", "")
                .trim()
                .to_string();
            if !description.is_empty() {
                return Ok(InputIntent::Synthesize { description });
            }
        }

        // Check for approval patterns
        if input.starts_with("approve") || input.starts_with("accept") {
            let rest = input
                .replace("approve", "")
                .replace("accept", "")
                .trim()
                .to_string();
            return Ok(InputIntent::Approval {
                request_id: rest,
                approved: true,
            });
        }
        if input.starts_with("reject") || input.starts_with("deny") {
            let rest = input
                .replace("reject", "")
                .replace("deny", "")
                .trim()
                .to_string();
            return Ok(InputIntent::Approval {
                request_id: rest,
                approved: false,
            });
        }

        // Check for 'details N' command - show full server info
        if input.starts_with("details") || input.starts_with("detail") {
            let rest = input
                .replace("details", "")
                .replace("detail", "")
                .trim()
                .to_string();
            if let Ok(num) = rest.parse::<usize>() {
                return Ok(InputIntent::Details { index: num });
            }
        }

        // Check for 'more' command - show all results
        if input == "more" || input == "show more" || input == "all" {
            return Ok(InputIntent::ShowMore);
        }

        // Check for 'explore N' command - explore documentation for API links
        if input.starts_with("explore") || input.starts_with("introspect") {
            let rest = input
                .replace("explore", "")
                .replace("introspect", "")
                .trim()
                .to_string();
            if let Ok(num) = rest.parse::<usize>() {
                return Ok(InputIntent::Explore { index: num });
            }
        }

        // Check for 'back' command - return to previous view
        if input == "back" || input == "return" || input == "list" {
            return Ok(InputIntent::Back);
        }

        // Check for option selection (numbers or letters)
        if let Ok(num) = input.parse::<usize>() {
            return Ok(InputIntent::SelectOption {
                option_id: num.to_string(),
            });
        }
        if input.len() == 1 && input.chars().next().unwrap().is_alphabetic() {
            return Ok(InputIntent::SelectOption {
                option_id: input.to_string(),
            });
        }

        // Check for refinement (longer input is likely a new goal)
        if raw_input.len() > 10 {
            return Ok(InputIntent::RefineGoal {
                new_goal: raw_input.to_string(),
            });
        }

        // Default: unclear
        Ok(InputIntent::Unclear {
            raw_input: raw_input.to_string(),
        })
    }
}

// ============================================================================
// LLM Entity (LLM acting as the external entity)
// ============================================================================

use crate::arbiter::llm_provider::LlmProvider;

/// LLM entity - another LLM acting as the dialogue partner
pub struct LlmEntity {
    /// The LLM provider
    provider: Arc<dyn LlmProvider>,
    /// System prompt for the LLM
    system_prompt: String,
    /// Conversation history for context
    history: Vec<(String, String)>, // (role, content)
    /// Name/persona
    name: String,
}

impl LlmEntity {
    pub fn new(provider: Arc<dyn LlmProvider>, name: &str, persona: Option<&str>) -> Self {
        let system_prompt = persona
            .unwrap_or(
                "You are a helpful assistant helping CCOS (Cognitive Capability Operating System) \
             plan tasks. When asked about goals, provide clear, concise responses. \
             If given options, choose the most practical one. \
             If you need more information, ask specific questions. \
             Keep responses brief and actionable.",
            )
            .to_string();

        Self {
            provider,
            system_prompt,
            history: Vec::new(),
            name: name.to_string(),
        }
    }

    pub fn with_persona(provider: Arc<dyn LlmProvider>, name: &str, persona: &str) -> Self {
        Self::new(provider, name, Some(persona))
    }
}

#[async_trait]
impl DialogueEntity for LlmEntity {
    fn entity_type(&self) -> &str {
        "llm"
    }

    async fn send(&self, message: &str) -> Result<(), EntityError> {
        // LLM entity just records the message for context
        // (actual send happens in receive when we need response)
        // For logging purposes:
        log::debug!("[LlmEntity:{}] CCOS said: {}", self.name, message);
        Ok(())
    }

    async fn receive(&self, _timeout: Option<Duration>) -> Result<String, EntityError> {
        // Build the conversation for the LLM
        let mut messages = vec![("system".to_string(), self.system_prompt.clone())];

        // Add history
        for (role, content) in &self.history {
            messages.push((role.clone(), content.clone()));
        }

        // Create prompt for LLM
        let prompt = messages
            .iter()
            .map(|(role, content)| format!("{}: {}", role, content))
            .collect::<Vec<_>>()
            .join("\n\n");

        // Call the LLM
        let response = self
            .provider
            .generate_text(&prompt)
            .await
            .map_err(|e| EntityError::LlmError(e.to_string()))?;

        log::debug!("[LlmEntity:{}] responded: {}", self.name, response);

        Ok(response)
    }

    async fn exchange(
        &self,
        message: &str,
        timeout: Option<Duration>,
    ) -> Result<String, EntityError> {
        // For LLM, we need to properly record the exchange
        // Record CCOS message
        let mut entity = self.clone_with_history();
        entity
            .history
            .push(("ccos".to_string(), message.to_string()));

        // Get LLM response
        let response = entity.receive(timeout).await?;

        Ok(response)
    }

    async fn parse_intent(&self, raw_input: &str) -> Result<InputIntent, EntityError> {
        // For LLM responses, we can use structured parsing
        // or just treat it as a refined goal/info

        let input = raw_input.trim().to_lowercase();

        // Check for clear signals
        if input.contains("proceed") || input.contains("continue") || input.contains("yes") {
            return Ok(InputIntent::Proceed);
        }

        if input.contains("cancel") || input.contains("abort") || input.contains("stop") {
            return Ok(InputIntent::Abandon {
                reason: Some("LLM requested".to_string()),
            });
        }

        // Check if it's selecting an option
        if let Some(option) = extract_option_selection(raw_input) {
            return Ok(InputIntent::SelectOption { option_id: option });
        }

        // Default: treat as goal refinement or additional info
        Ok(InputIntent::RefineGoal {
            new_goal: raw_input.to_string(),
        })
    }
}

impl LlmEntity {
    fn clone_with_history(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            system_prompt: self.system_prompt.clone(),
            history: self.history.clone(),
            name: self.name.clone(),
        }
    }
}

impl Clone for LlmEntity {
    fn clone(&self) -> Self {
        self.clone_with_history()
    }
}

// Helper function to extract option selection from LLM response
fn extract_option_selection(response: &str) -> Option<String> {
    let lower = response.to_lowercase();

    // Check for "option A", "choice 1", etc.
    let patterns = [
        ("option ", true),
        ("choice ", true),
        ("select ", true),
        ("choosing ", true),
        ("i choose ", true),
        ("i'll go with ", true),
    ];

    for (pattern, _) in &patterns {
        if let Some(idx) = lower.find(pattern) {
            let rest = &response[idx + pattern.len()..];
            let option: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
            if !option.is_empty() {
                return Some(option);
            }
        }
    }

    None
}

// ============================================================================
// Boxed Entity Helper
// ============================================================================

/// Creates a boxed human entity
pub fn human_entity(name: Option<&str>) -> Box<dyn DialogueEntity> {
    Box::new(HumanEntity::new(name.map(|s| s.to_string())))
}

/// Creates a boxed LLM entity
pub fn llm_entity(provider: Arc<dyn LlmProvider>, name: &str) -> Box<dyn DialogueEntity> {
    Box::new(LlmEntity::new(provider, name, None))
}
