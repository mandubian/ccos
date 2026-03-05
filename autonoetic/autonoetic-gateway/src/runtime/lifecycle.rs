//! Agent Execution Lifecycle.
//!
//! Manages Wake -> Context Assembly -> Reasoning -> Hibernate.

use crate::llm::{CompletionRequest, LlmDriver, Message, Role};
use crate::runtime::guard::LoopGuard;
use autonoetic_types::agent::AgentManifest;

pub struct AgentExecutor {
    pub manifest: AgentManifest,
    pub instructions: String,
    pub llm: std::sync::Arc<dyn LlmDriver>,
    pub guard: LoopGuard,
}

impl AgentExecutor {
    pub fn new(
        manifest: AgentManifest,
        instructions: String,
        llm: std::sync::Arc<dyn LlmDriver>,
    ) -> Self {
        Self {
            manifest,
            instructions,
            llm,
            guard: LoopGuard::new(5), // allow 5 pure reasoning loops before bail
        }
    }

    /// Run the agent loop until completion or guard trip.
    pub async fn execute_loop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Agent {} waking up...", self.manifest.agent.id);

        loop {
            // 1. Loop Guard Check
            self.guard.check_loop()?;

            // 2. Context Assembly
            tracing::debug!("Assembling context");
            
            let req = CompletionRequest {
                model: self.manifest.llm_config.as_ref().map(|c| c.model.clone()).unwrap_or_else(|| "gpt-4o".to_string()),
                messages: vec![
                    Message { role: Role::System, content: self.instructions.clone() },
                    Message { role: Role::User, content: "What is your next action?".to_string() }
                ],
                max_tokens: self.manifest.limits.as_ref().and_then(|l| Some(l.max_execution_time_sec as u32 * 10)), // stub logic
                temperature: self.manifest.llm_config.as_ref().map(|c| c.temperature as f32),
            };

            // 3. Reasoning (LLM call)
            tracing::debug!("Calling LLM");
            let _response = self.llm.complete(&req).await?;

            // 4. Action Execution (Stubbed for now)
            // If the response contains a tool call that modifies state or invokes an action:
            // self.guard.register_progress(); 
            // break; (if task complete)
            
            // For now, we'll just break immediately as this is scaffolding
            tracing::info!("Agent {} hibernating...", self.manifest.agent.id);
            break;
        }

        Ok(())
    }
}
