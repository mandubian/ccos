// DialoguePlanner: Conversational planning that builds plans through dialogue
#![allow(unused_assignments)]

use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use super::entity::{DialogueEntity, EntityError};
use super::turn_processor::{ProcessingError, TurnProcessor};
use super::types::*;

use crate::ccos_core::CCOS;
use crate::planner::modular_planner::PlanResult;

/// A planner that builds plans through dialogue with external entities
pub struct DialoguePlanner {
    /// Unique ID for this dialogue session
    id: DialogueId,
    /// The entity we're dialoguing with
    entity: Box<dyn DialogueEntity>,
    /// Configuration
    config: DialogueConfig,
    /// CCOS core for access to marketplace, etc.
    ccos: Arc<CCOS>,
    /// Turn processor for discovery, connect, synthesis
    turn_processor: TurnProcessor,
    /// Conversation history
    turns: Vec<ConversationTurn>,
    /// Current state
    state: DialogueState,
    /// Current goal (may be refined during dialogue)
    current_goal: String,
    /// Current analysis (updated as dialogue progresses)
    current_analysis: Option<GoalAnalysis>,
    /// Start time
    started_at: Instant,
}

impl DialoguePlanner {
    /// Create a new dialogue planner
    pub fn new(entity: Box<dyn DialogueEntity>, ccos: Arc<CCOS>, config: DialogueConfig) -> Self {
        let id = Uuid::new_v4().to_string();

        // Create turn processor with CCOS components
        let turn_processor =
            TurnProcessor::new(ccos.get_capability_marketplace(), ccos.get_intent_graph())
                .with_llm_provider(ccos.cognitive_engine.get_llm_provider_arc());

        Self {
            id,
            entity,
            config,
            ccos,
            turn_processor,
            turns: Vec::new(),
            state: DialogueState::Analyzing,
            current_goal: String::new(),
            current_analysis: None,
            started_at: Instant::now(),
        }
    }

    /// Create with default config
    pub fn with_defaults(entity: Box<dyn DialogueEntity>, ccos: Arc<CCOS>) -> Self {
        Self::new(entity, ccos, DialogueConfig::default())
    }

    /// Get reference to the underlying CCOS instance
    pub fn get_ccos(&self) -> &Arc<CCOS> {
        &self.ccos
    }
    /// Main entry point: converse until we have a plan or abandon
    pub async fn converse(&mut self, initial_goal: &str) -> Result<DialogueResult, DialogueError> {
        self.current_goal = initial_goal.to_string();

        // Record system turn for start
        self.record_turn(
            Speaker::System,
            &format!("Dialogue started. Goal: {}", initial_goal),
            vec![],
        );

        // Initial analysis
        self.state = DialogueState::Analyzing;
        let analysis = self.analyze_goal(initial_goal).await?;

        // Record analysis action
        self.record_turn(
            Speaker::System,
            "Goal analyzed",
            vec![TurnAction::GoalAnalyzed {
                feasibility: analysis.feasibility,
                missing_domains: analysis.missing_domains.clone(),
                suggestions_count: analysis.suggestions.len(),
            }],
        );

        self.current_analysis = Some(analysis.clone());

        // Check if we can proceed immediately (high feasibility, simple goal)
        if analysis.can_proceed_immediately && self.config.autonomy == AutonomyLevel::Autonomous {
            // Skip dialogue, plan directly
            // TODO: Call planner here when mutable borrow issue is resolved
            return Err(DialogueError::AnalysisFailed(
                "Direct planning not yet implemented - use dialogue loop".to_string(),
            ));
        }

        // Generate initial message to entity
        let initial_message = self.generate_initial_message(&analysis);
        self.send_and_record(&initial_message).await?;

        // Enter dialogue loop
        let mut turn_count = 0;
        loop {
            // Check turn limit
            turn_count += 1;
            if turn_count > self.config.max_turns {
                self.state = DialogueState::Abandoned {
                    reason: "Maximum turns reached".to_string(),
                };
                return Ok(DialogueResult::Abandoned {
                    reason: "Maximum turns reached".to_string(),
                    history: self.get_history(),
                });
            }

            // Wait for entity input
            self.state = DialogueState::WaitingForInput;
            let input = match self
                .entity
                .receive(Some(Duration::from_secs(self.config.turn_timeout_secs)))
                .await
            {
                Ok(input) => input,
                Err(EntityError::Timeout) => {
                    self.send_and_record(
                        "Still there? Type 'quit' to exit or continue with your response.",
                    )
                    .await?;
                    continue;
                }
                Err(EntityError::Cancelled) => {
                    self.state = DialogueState::Abandoned {
                        reason: "Entity cancelled".to_string(),
                    };
                    return Ok(DialogueResult::Abandoned {
                        reason: "Entity cancelled".to_string(),
                        history: self.get_history(),
                    });
                }
                Err(e) => return Err(DialogueError::EntityError(e.to_string())),
            };

            // Parse entity intent
            let intent = self.entity.parse_intent(&input).await?;

            // Record entity turn
            self.record_turn(Speaker::Entity, &input, vec![]);

            // Process the intent
            self.state = DialogueState::Processing;
            let result = self.process_intent(&intent).await?;

            // Record actions
            if !result.actions.is_empty() {
                // Update last entity turn with actions
                if let Some(last_turn) = self.turns.last_mut() {
                    last_turn.actions = result.actions.clone();
                }
            }

            // Handle goal refinement - need to re-analyze
            for action in &result.actions {
                if let TurnAction::IntentRefined {
                    new_description, ..
                } = action
                {
                    self.current_goal = new_description.clone();
                    let new_analysis = self.analyze_goal(&self.current_goal).await?;
                    self.current_analysis = Some(new_analysis);
                }
            }

            // Check for completion
            if let Some(completed) = result.completed_plan {
                self.state = DialogueState::Completed {
                    plan_id: completed.plan_id.clone(),
                };

                // Create a minimal PlanResult to return
                let plan = create_plan_result_from_completed(&completed);

                return Ok(DialogueResult::PlanGenerated {
                    plan,
                    history: self.get_history(),
                });
            }

            // Check for abandonment
            if !result.should_continue {
                self.state = DialogueState::Abandoned {
                    reason: result.abandon_reason.clone().unwrap_or_default(),
                };
                return Ok(DialogueResult::Abandoned {
                    reason: result.abandon_reason.unwrap_or_default(),
                    history: self.get_history(),
                });
            }

            // Send next message
            if let Some(message) = result.next_message {
                self.send_and_record(&message).await?;
            }
        }
    }

    /// Process entity intent
    async fn process_intent(
        &self,
        intent: &InputIntent,
    ) -> Result<ProcessingResult, ProcessingError> {
        let analysis = self
            .current_analysis
            .as_ref()
            .ok_or_else(|| ProcessingError::PlanningFailed("No analysis available".to_string()))?;

        // Simple processing without TurnProcessor for now
        let mut actions = Vec::new();
        let mut next_message = None;
        let mut completed_plan = None;
        let should_continue;
        let abandon_reason;

        match intent {
            InputIntent::RefineGoal { new_goal } => {
                actions.push(TurnAction::IntentRefined {
                    intent_id: "root".to_string(),
                    old_description: self.current_goal.clone(),
                    new_description: new_goal.clone(),
                });
                next_message = Some(format!(
                    "Got it! Updating goal to: \"{}\"\nLet me re-analyze...",
                    new_goal
                ));
                should_continue = true;
                abandon_reason = None;
            }

            InputIntent::Proceed => {
                if analysis.feasibility >= 0.8 {
                    // Generate plan
                    completed_plan = Some(CompletedPlan {
                        rtfs_plan: format!(
                            ";; Plan for: {}\n(do\n  ; Steps would go here\n)",
                            self.current_goal
                        ),
                        intent_ids: vec!["root".to_string()],
                        plan_id: Some(format!("plan-{}", uuid::Uuid::new_v4())),
                        conversation_summary: "Plan generated through dialogue".to_string(),
                    });
                    next_message = Some("Plan generated successfully!".to_string());
                    should_continue = false;
                } else {
                    next_message = Some(format!(
                        "Cannot proceed yet. Feasibility is {:.0}%, missing: {:?}",
                        analysis.feasibility * 100.0,
                        analysis.missing_domains
                    ));
                    should_continue = true;
                }
                abandon_reason = None;
            }

            InputIntent::Abandon { reason } => {
                should_continue = false;
                abandon_reason = reason.clone();
                next_message = Some("Dialogue ended.".to_string());
            }

            InputIntent::SelectOption { option_id: _ }
            | InputIntent::Details { .. }
            | InputIntent::ShowMore
            | InputIntent::Back
            | InputIntent::Explore { .. }
            | InputIntent::Discover { .. }
            | InputIntent::ConnectServer { .. }
            | InputIntent::Synthesize { .. }
            | InputIntent::Approval { .. }
            | InputIntent::Question { .. }
            | InputIntent::ProvideInfo { .. }
            | InputIntent::Unclear { .. } => {
                // Delegate to TurnProcessor for actual discovery/connect/synthesis/details
                let result = self
                    .turn_processor
                    .process(intent, &self.current_goal, analysis, &self.config)
                    .await?;

                // Merge actions from turn processor
                actions.extend(result.actions);
                next_message = result.next_message;
                completed_plan = result.completed_plan;
                should_continue = result.should_continue;
                abandon_reason = result.abandon_reason;
            }
        }

        Ok(ProcessingResult {
            actions,
            next_message,
            completed_plan,
            should_continue,
            abandon_reason,
        })
    }

    /// Analyze goal feasibility
    async fn analyze_goal(&self, goal: &str) -> Result<GoalAnalysis, DialogueError> {
        // Use LLM to identify required domains
        let required_domains = self.infer_required_domains(goal).await?;

        // Get available domains from marketplace
        let available_domains = self.get_available_domains().await;

        // Compute missing
        let missing_domains: Vec<String> = required_domains
            .iter()
            .filter(|d| {
                !available_domains
                    .iter()
                    .any(|a| a.to_lowercase() == d.to_lowercase())
            })
            .cloned()
            .collect();

        // Calculate feasibility
        let feasibility = if required_domains.is_empty() {
            1.0
        } else {
            (required_domains.len() - missing_domains.len()) as f32 / required_domains.len() as f32
        };

        // Generate suggestions
        let suggestions = self.generate_suggestions(&missing_domains, &available_domains);

        // Can proceed immediately if high feasibility and autonomous
        let can_proceed = feasibility >= 0.8;

        Ok(GoalAnalysis {
            goal: goal.to_string(),
            required_domains,
            available_domains,
            missing_domains,
            feasibility,
            suggestions,
            can_proceed_immediately: can_proceed,
        })
    }

    /// Infer required domains from goal text using LLM
    async fn infer_required_domains(&self, goal: &str) -> Result<Vec<String>, DialogueError> {
        // Try LLM-based intent analysis for better query generation
        match crate::discovery::LlmDiscoveryService::new().await {
            Ok(llm_discovery) => {
                match llm_discovery.analyze_goal(goal).await {
                    Ok(analysis) => {
                        log::info!(
                            "LLM intent analysis: action={}, target={}, confidence={}",
                            analysis.primary_action,
                            analysis.target_object,
                            analysis.confidence
                        );

                        // Use expanded_queries as the "domains" for discovery
                        // These are much better search terms than single keywords
                        if !analysis.expanded_queries.is_empty() {
                            return Ok(analysis.expanded_queries);
                        }

                        // Fallback: use domain_keywords
                        if !analysis.domain_keywords.is_empty() {
                            return Ok(analysis.domain_keywords);
                        }

                        // Last resort: use target_object
                        Ok(vec![analysis.target_object])
                    }
                    Err(e) => {
                        log::warn!("LLM analysis failed, falling back to keywords: {}", e);
                        self.fallback_domain_inference(goal)
                    }
                }
            }
            Err(e) => {
                log::warn!("LLM not available, using keyword fallback: {}", e);
                self.fallback_domain_inference(goal)
            }
        }
    }

    /// Fallback keyword-based domain inference when LLM is unavailable
    fn fallback_domain_inference(&self, goal: &str) -> Result<Vec<String>, DialogueError> {
        let goal_lower = goal.to_lowercase();
        let mut domains = Vec::new();

        // Domain keywords mapping (simple fallback)
        let domain_keywords: Vec<(&str, Vec<&str>)> = vec![
            (
                "cryptocurrency exchange API",
                vec![
                    "trade", "trading", "buy", "sell", "bitcoin", "crypto", "exchange",
                ],
            ),
            (
                "news API feed",
                vec!["news", "headlines", "articles", "feed", "rss"],
            ),
            (
                "stock market data API",
                vec!["price", "ticker", "quote", "market", "stock", "chart"],
            ),
            (
                "file system operations",
                vec!["file", "directory", "read", "write", "delete", "folder"],
            ),
            (
                "github repository API",
                vec![
                    "github",
                    "repo",
                    "repository",
                    "code",
                    "git",
                    "commit",
                    "issue",
                    "pull request",
                ],
            ),
            (
                "email messaging API",
                vec!["email", "mail", "send", "inbox", "message"],
            ),
            (
                "http web API",
                vec!["http", "api", "request", "fetch", "url", "web"],
            ),
        ];

        for (domain, keywords) in domain_keywords {
            if keywords.iter().any(|kw| goal_lower.contains(kw)) {
                domains.push(domain.to_string());
            }
        }

        // If no domains inferred, use the goal itself as search query
        if domains.is_empty() {
            // Extract key terms from the goal
            let stopwords = [
                "the", "a", "an", "for", "in", "on", "to", "and", "or", "of", "with", "i", "want",
                "need",
            ];
            let keywords: Vec<&str> = goal_lower
                .split_whitespace()
                .filter(|w| w.len() > 2 && !stopwords.contains(w))
                .collect();

            if !keywords.is_empty() {
                domains.push(keywords.join(" "));
            } else {
                domains.push(goal.to_string());
            }
        }

        Ok(domains)
    }

    /// Get available domains from capability marketplace
    async fn get_available_domains(&self) -> Vec<String> {
        // Get capabilities from marketplace
        let marketplace = self.ccos.get_capability_marketplace();
        let capabilities = marketplace.list_capabilities().await;

        // Extract unique domains from all capabilities
        let mut domains: std::collections::HashSet<String> = std::collections::HashSet::new();

        for cap in capabilities {
            // Add all domains from the capability's domains field
            for domain in &cap.domains {
                domains.insert(domain.clone());
            }

            // Also extract domain prefix from capability ID (e.g., "ccos.fs.read" -> "fs")
            if let Some(domain_part) = cap.id.split('.').nth(1) {
                domains.insert(domain_part.to_string());
            }
        }

        // Always include some base domains
        domains.insert("general".to_string());

        domains.into_iter().collect()
    }

    /// Generate suggestions based on analysis
    fn generate_suggestions(
        &self,
        missing_domains: &[String],
        available_domains: &[String],
    ) -> Vec<Suggestion> {
        let mut suggestions = Vec::new();

        // Suggest discovery for each missing domain
        for domain in missing_domains {
            suggestions.push(Suggestion::Discover {
                domain: domain.clone(),
                example_servers: vec![format!("{}-server", domain)],
            });
        }

        // If we have some available domains, suggest using them
        if !available_domains.is_empty() && !missing_domains.is_empty() {
            let available_list = available_domains.join(", ");
            suggestions.push(Suggestion::RefineGoal {
                alternative: format!(
                    "Goal achievable with available capabilities: {}",
                    available_list
                ),
                reason: "Some required capabilities are not available".to_string(),
            });
        }

        suggestions
    }

    /// Generate initial message to entity
    fn generate_initial_message(&self, analysis: &GoalAnalysis) -> String {
        let mut message = format!("Analyzing goal: \"{}\"\n\n", analysis.goal);

        message.push_str(&format!(
            "📊 Feasibility: {:.0}%\n",
            analysis.feasibility * 100.0
        ));

        if !analysis.missing_domains.is_empty() {
            message.push_str(&format!(
                "❌ Missing: {}\n",
                analysis.missing_domains.join(", ")
            ));
        }

        if !analysis.available_domains.is_empty() {
            message.push_str(&format!(
                "✅ Available: {}\n",
                analysis.available_domains.join(", ")
            ));
        }

        if !analysis.suggestions.is_empty() {
            message.push_str("\n📋 Options:\n");
            for (i, suggestion) in analysis.suggestions.iter().enumerate() {
                let desc = match suggestion {
                    Suggestion::Discover { domain, .. } => {
                        format!("Discover '{}' capabilities", domain)
                    }
                    Suggestion::Synthesize { description, .. } => {
                        format!("Synthesize: {}", description)
                    }
                    Suggestion::RefineGoal { alternative, .. } => {
                        format!("Refine goal: {}", alternative)
                    }
                    Suggestion::ConnectServer { server_name, .. } => {
                        format!("Connect to {}", server_name)
                    }
                };
                message.push_str(&format!("  [{}] {}\n", i + 1, desc));
            }
        }

        message.push_str("\nWhat would you like to do? (Enter number, or type your response)");

        message
    }

    /// Send message to entity and record turn
    async fn send_and_record(&mut self, message: &str) -> Result<(), DialogueError> {
        self.entity
            .send(message)
            .await
            .map_err(|e| DialogueError::EntityError(e.to_string()))?;

        self.record_turn(Speaker::Ccos, message, vec![]);

        Ok(())
    }

    /// Record a turn in history
    fn record_turn(&mut self, speaker: Speaker, message: &str, actions: Vec<TurnAction>) {
        self.turns.push(ConversationTurn {
            index: self.turns.len(),
            speaker,
            message: message.to_string(),
            actions,
            timestamp: Some(Instant::now()),
        });
    }

    /// Get dialogue history
    fn get_history(&self) -> DialogueHistory {
        DialogueHistory {
            id: self.id.clone(),
            goal: self.current_goal.clone(),
            turns: self.turns.clone(),
            final_state: self.state.clone(),
            duration_ms: self.started_at.elapsed().as_millis() as u64,
            result_plan_id: match &self.state {
                DialogueState::Completed { plan_id } => plan_id.clone(),
                _ => None,
            },
        }
    }

    /// Get current dialogue ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get current state
    pub fn state(&self) -> &DialogueState {
        &self.state
    }

    /// Get turn count
    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }
}

/// Create a minimal PlanResult from CompletedPlan
fn create_plan_result_from_completed(completed: &CompletedPlan) -> PlanResult {
    use crate::planner::modular_planner::orchestrator::PlanningTrace;
    use std::collections::HashMap;
    PlanResult {
        root_intent_id: "root".to_string(),
        intent_ids: completed.intent_ids.clone(),
        sub_intents: vec![],
        resolutions: HashMap::new(),
        rtfs_plan: completed.rtfs_plan.clone(),
        trace: PlanningTrace::default(),
        plan_status: crate::types::PlanStatus::Draft,
        plan_id: completed.plan_id.clone(),
        archive_hash: None,
        archive_path: None,
    }
}

/// Result of a dialogue session
#[derive(Debug)]
pub enum DialogueResult {
    /// Successfully generated a plan
    PlanGenerated {
        plan: PlanResult,
        history: DialogueHistory,
    },
    /// Dialogue was abandoned
    Abandoned {
        reason: String,
        history: DialogueHistory,
    },
}

/// Errors during dialogue
#[derive(Debug, thiserror::Error)]
pub enum DialogueError {
    #[error("Entity error: {0}")]
    EntityError(String),

    #[error("Processing error: {0}")]
    ProcessingError(#[from] ProcessingError),

    #[error("Analysis failed: {0}")]
    AnalysisFailed(String),
}

impl From<EntityError> for DialogueError {
    fn from(err: EntityError) -> Self {
        DialogueError::EntityError(err.to_string())
    }
}
