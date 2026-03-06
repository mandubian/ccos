//! Agent Execution Lifecycle.
//!
//! Manages Wake -> Context Assembly -> Reasoning -> Act -> Hibernate.

use crate::llm::{CompletionRequest, LlmDriver, Message, StopReason};
use crate::runtime::guard::LoopGuard;
use crate::runtime::mcp::McpToolRuntime;
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
            guard: LoopGuard::new(5), // bail after 5 non-progressing cycles
        }
    }

    /// Run the agent loop until completion or guard trip.
    pub async fn execute_loop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Agent {} waking up...", self.manifest.agent.id);

        let mut mcp_runtime = McpToolRuntime::from_env().await?;
        if !mcp_runtime.is_empty() {
            tracing::info!("Loaded MCP tool runtime for agent {}", self.manifest.agent.id);
        }

        let model = self.manifest
            .llm_config.as_ref()
            .map(|c| c.model.clone())
            .unwrap_or_else(|| "gpt-4o".to_string());

        let temperature = self.manifest.llm_config.as_ref().map(|c| c.temperature as f32);

        // Conversation history (grows with tool call results)
        let mut history: Vec<Message> = vec![
            Message::system(self.instructions.clone()),
            Message::user("What is your next action?"),
        ];

        loop {
            // --- Loop Guard ---
            self.guard.check_loop()?;

            // --- Context Assembly ---
            tracing::debug!("Assembling context ({} messages)", history.len());

            let tools = mcp_runtime.tool_definitions()?;

            let req = CompletionRequest {
                model: model.clone(),
                messages: history.clone(),
                tools,
                max_tokens: None,
                temperature,
            };

            // --- Reasoning (LLM call) ---
            tracing::debug!("Calling LLM");
            let response = self.llm.complete(&req).await?;

            tracing::debug!(
                stop_reason = ?response.stop_reason,
                text_len = response.text.len(),
                tool_calls = response.tool_calls.len(),
                "LLM response received"
            );

            match response.stop_reason {
                StopReason::ToolUse => {
                    // Push the assistant's tool-call turn into history
                    let mut assistant_msg = Message::assistant(response.text.clone());
                    assistant_msg.tool_calls = response.tool_calls.clone();
                    history.push(assistant_msg);

                    // Execute each tool call (stubbed for now)
                    for tc in &response.tool_calls {
                        tracing::info!(tool = tc.name, args = tc.arguments, "Agent requested tool call");
                        let result = if mcp_runtime.has_tool(&tc.name) {
                            tracing::debug!(tool = tc.name, "Dispatching tool call to MCP runtime");
                            mcp_runtime.call_tool(&tc.name, &tc.arguments).await?
                        } else {
                            // TODO: dispatch non-MCP tools to sandbox/capability runtime.
                            format!("Tool '{}' result placeholder", tc.name)
                        };
                        history.push(Message::tool_result(tc.id.clone(), tc.name.clone(), result));
                    }

                    // Meaningful action taken — reset the guard
                    self.guard.register_progress();
                }
                StopReason::EndTurn | StopReason::StopSequence => {
                    tracing::info!("Agent {} task complete, hibernating", self.manifest.agent.id);
                    break;
                }
                StopReason::MaxTokens => {
                    tracing::warn!("Agent {} exceeded max tokens", self.manifest.agent.id);
                    break;
                }
                StopReason::Other(ref reason) => {
                    tracing::warn!("Agent {} stopped: {}", self.manifest.agent.id, reason);
                    break;
                }
            }
        }

        Ok(())
    }
}
