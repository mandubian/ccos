//! Context Horizon Management
//!
//! This module implements the Context Horizon Manager that addresses the finite
//! context window of the core Arbiter LLM through virtualization and distillation.

use super::types::Intent;
use super::types::IntentId;
use crate::ccos::working_memory::backend::QueryParams;
use crate::ccos::working_memory::boundaries::{Boundary, BoundaryType};
use crate::runtime::error::RuntimeError;
use std::collections::HashMap;

use std::sync::{Arc, Mutex};

// Integration: Working Memory (new modular API)
use crate::ccos::working_memory::backend_inmemory::InMemoryJsonlBackend;
use crate::ccos::working_memory::facade::WorkingMemory;
use crate::ccos::working_memory::types::WorkingMemoryEntry;

// Minimal AbstractStep and ResourceId types to resolve missing type errors
#[derive(Clone, Debug)]
pub struct AbstractStep {
    pub name: String,
}

pub type ResourceId = String;

// Minimal ContextKey type to resolve missing type errors
pub type ContextKey = String;

// Minimal placeholder types for missing imports
#[derive(Clone, Debug)]
pub struct DistilledWisdom {
    pub content: String,
}

impl DistilledWisdom {
    pub fn new() -> Self {
        Self {
            content: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AbstractPlan {
    pub steps: Vec<String>,
    pub data_handles: Vec<String>,
}

impl AbstractPlan {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            data_handles: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Context {
    pub data: std::collections::HashMap<String, String>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            data: std::collections::HashMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Task {
    pub name: String,
    pub description: String,
}

impl Task {
    pub fn new(name: String, description: String) -> Self {
        Self { name, description }
    }
}

/// Main Context Horizon Manager
pub struct ContextHorizonManager {
    intent_graph: IntentGraphVirtualization,
    causal_chain: CausalChainDistillation,
    plan_abstraction: PlanAbstraction,
    config: ContextHorizonConfig,
    // Working Memory store (in-memory + optional file-backed)
    working_memory: Arc<Mutex<WorkingMemory>>,
}

impl ContextHorizonManager {
    pub fn new() -> Result<Self, RuntimeError> {
        // Initialize Working Memory using new facade + in-memory backend (no disk by default)
        // If you want persistence, pass Some(PathBuf) to InMemoryJsonlBackend::new.
        let backend = InMemoryJsonlBackend::new(None, Some(10_000), Some(500_000));
        let wm = WorkingMemory::new(Box::new(backend));

        Ok(Self {
            intent_graph: IntentGraphVirtualization::new(),
            causal_chain: CausalChainDistillation::new(),
            plan_abstraction: PlanAbstraction::new(),
            config: ContextHorizonConfig::default(),
            working_memory: Arc::new(Mutex::new(wm)),
        })
    }

    /// Load relevant context for a task while respecting context horizon constraints
    pub fn load_relevant_context(&self, task: &Task) -> Result<Context, RuntimeError> {
        // 1. Semantic search for relevant intents
        let relevant_intents = self.intent_graph.find_relevant_intents(task)?;

        // 2. Load distilled causal chain wisdom
        let distilled_wisdom = self.causal_chain.get_distilled_wisdom()?;

        // Persist distilled wisdom into Working Memory for future recall
        self.persist_wisdom_to_working_memory(task, &distilled_wisdom)?;

        // 3. Create abstract plan
        let abstract_plan = self.plan_abstraction.create_abstract_plan(task)?;

        // 4. Apply context horizon constraints
        let constrained_context =
            self.apply_context_constraints(relevant_intents, distilled_wisdom, abstract_plan)?;

        Ok(constrained_context)
    }

    /// Apply context horizon constraints to keep within limits
    fn apply_context_constraints(
        &self,
        intents: Vec<Intent>,
        wisdom: DistilledWisdom,
        plan: AbstractPlan,
    ) -> Result<Context, RuntimeError> {
        let mut context = Context::new();

        // Estimate token usage
        let intent_tokens = self.estimate_intent_tokens(&intents);
        let wisdom_tokens = self.estimate_wisdom_tokens(&wisdom);
        let plan_tokens = self.estimate_plan_tokens(&plan);

        let total_tokens = intent_tokens + wisdom_tokens + plan_tokens;

        if total_tokens > self.config.max_tokens {
            // Apply reduction strategies
            let reduced_intents = self.reduce_intents(intents, self.config.max_intent_tokens)?;
            let reduced_wisdom = self.reduce_wisdom(wisdom, self.config.max_wisdom_tokens)?;
            let reduced_plan = self.reduce_plan(plan, self.config.max_plan_tokens)?;

            context
                .data
                .insert("intents".to_string(), format!("{:?}", reduced_intents));
            context
                .data
                .insert("wisdom".to_string(), format!("{:?}", reduced_wisdom));
            context
                .data
                .insert("plan".to_string(), format!("{:?}", reduced_plan));
        } else {
            context
                .data
                .insert("intents".to_string(), format!("{:?}", intents));
            context
                .data
                .insert("wisdom".to_string(), format!("{:?}", wisdom));
            context
                .data
                .insert("plan".to_string(), format!("{:?}", plan));
        }

        Ok(context)
    }

    /// Estimate token count for intents
    fn estimate_intent_tokens(&self, intents: &[Intent]) -> usize {
        let mut total_tokens = 0;

        for intent in intents {
            // Rough token estimation: ~4 characters per token
            total_tokens += intent.goal.len() / 4;
            total_tokens += intent.constraints.len() * 10; // ~10 tokens per constraint
            total_tokens += intent.preferences.len() * 8; // ~8 tokens per preference

            if intent.success_criteria.is_some() {
                total_tokens += 20; // ~20 tokens for success criteria
            }
        }

        total_tokens
    }

    /// Estimate token count for wisdom
    fn estimate_wisdom_tokens(&self, wisdom: &DistilledWisdom) -> usize {
        let mut total_tokens = 0;

        // Agent reliability scores
        total_tokens += wisdom.content.len() / 4; // Rough token estimation

        // Failure patterns
        total_tokens += wisdom.content.len() / 4; // Rough token estimation

        // Optimized strategies
        total_tokens += wisdom.content.len() / 4; // Rough token estimation

        // Cost insights
        total_tokens += wisdom.content.len() / 4; // Rough token estimation

        // Performance metrics
        total_tokens += wisdom.content.len() / 4; // Rough token estimation

        total_tokens
    }

    /// Estimate token count for plan
    fn estimate_plan_tokens(&self, plan: &AbstractPlan) -> usize {
        let mut total_tokens = 0;

        // Abstract steps
        total_tokens += plan.steps.len() * 10 + plan.data_handles.len() * 5;

        // Data handles
        total_tokens += plan.data_handles.len() * 3;

        // Metadata
        total_tokens += plan.steps.len() * 10 + plan.data_handles.len() * 5;

        total_tokens
    }

    /// Reduce intents to fit within token limit
    fn reduce_intents(
        &self,
        intents: Vec<Intent>,
        max_tokens: usize,
    ) -> Result<Vec<Intent>, RuntimeError> {
        let mut reduced = Vec::new();
        let mut current_tokens = 0;

        // Sort by relevance score (assuming it's stored in metadata)
        let mut sorted_intents = intents;
        sorted_intents.sort_by(|a, b| {
            let score_a = a
                .metadata
                .get("relevance_score")
                .and_then(|v| v.as_number())
                .unwrap_or(0.0);
            let score_b = b
                .metadata
                .get("relevance_score")
                .and_then(|v| v.as_number())
                .unwrap_or(0.0);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for intent in sorted_intents {
            let intent_tokens = self.estimate_intent_tokens(&[intent.clone()]);

            if current_tokens + intent_tokens <= max_tokens {
                reduced.push(intent);
                current_tokens += intent_tokens;
            } else {
                break;
            }
        }

        Ok(reduced)
    }

    /// Reduce wisdom to fit within token limit
    fn reduce_wisdom(
        &self,
        wisdom: DistilledWisdom,
        max_tokens: usize,
    ) -> Result<DistilledWisdom, RuntimeError> {
        let mut reduced = DistilledWisdom::new();
        let mut current_tokens = 0;

        // Add most important agent reliability scores
        // Placeholder for agent reliability scores processing
        if current_tokens + 5 <= max_tokens {
            reduced.content.push_str("agent_reliability_score");
            current_tokens += 5;
        }

        // Add most recent failure patterns
        // Placeholder for failure patterns processing
        if current_tokens + 10 <= max_tokens {
            reduced.content.push_str("failure_pattern");
        }

        // Add most effective strategies
        // Placeholder for optimized strategies processing
        if current_tokens + 15 <= max_tokens {
            reduced.content.push_str("optimized_strategy");
        }

        Ok(reduced)
    }

    /// Reduce plan to fit within token limit
    fn reduce_plan(
        &self,
        plan: AbstractPlan,
        max_tokens: usize,
    ) -> Result<AbstractPlan, RuntimeError> {
        let mut reduced = AbstractPlan::new();
        let mut current_tokens = 0;

        // Add most important steps
        for step in &plan.steps {
            if current_tokens + 8 <= max_tokens {
                reduced.steps.push(step.clone());
                current_tokens += 8;
            } else {
                break;
            }
        }

        // Add essential data handles
        for handle in &plan.data_handles {
            if current_tokens + 3 <= max_tokens {
                reduced.data_handles.push(handle.clone());
                current_tokens += 3;
            } else {
                break;
            }
        }

        Ok(reduced)
    }

    /// Update context horizon configuration
    pub fn update_config(&mut self, config: ContextHorizonConfig) {
        self.config = config;
    }

    /// Access to Working Memory (for tests or external consumers)
    pub fn working_memory(&self) -> Arc<Mutex<WorkingMemory>> {
        Arc::clone(&self.working_memory)
    }

    /// Translate a Boundary into Working Memory QueryParams.
    /// Supports:
    /// - TimeLimit: constraints.from_ts (u64), constraints.to_ts (u64)
    /// - TokenLimit: constraints.max_tokens (usize) -> maps to limit heuristic at query stage
    fn boundary_to_query_params(&self, boundary: &Boundary) -> QueryParams {
        match boundary.boundary_type {
            BoundaryType::TimeLimit => {
                let from_ts = boundary.get_u64("from_ts");
                let to_ts = boundary.get_u64("to_ts");
                QueryParams::default().with_time_window(from_ts, to_ts)
            }
            BoundaryType::TokenLimit => {
                let limit = boundary.get_usize("max_tokens");
                QueryParams::default().with_limit(limit)
            }
            _ => QueryParams::default(),
        }
    }

    /// Fetch distilled wisdom entries from Working Memory honoring provided boundaries.
    /// Always enforces the "wisdom" tag; additional boundaries may add time windows or limits.
    pub fn fetch_wisdom_from_working_memory(
        &self,
        boundaries: &[Boundary],
    ) -> Result<Vec<crate::ccos::working_memory::types::WorkingMemoryEntry>, RuntimeError> {
        let wm = self
            .working_memory
            .lock()
            .map_err(|_| RuntimeError::Generic("WorkingMemory lock poisoned".into()))?;

        // Start with default wisdom tag and optional global limit/time window derived from boundaries
        let mut qp = QueryParams::with_tags(["wisdom"]);
        // Merge boundaries: last writer wins for overlapping fields
        for b in boundaries {
            let bqp = self.boundary_to_query_params(b);
            if bqp.from_ts_s.is_some() || bqp.to_ts_s.is_some() {
                qp = qp.with_time_window(bqp.from_ts_s, bqp.to_ts_s);
            }
            if bqp.limit.is_some() {
                qp = qp.with_limit(bqp.limit);
            }
        }

        let res = wm
            .query(&qp)
            .map_err(|e| RuntimeError::Generic(format!("WorkingMemory query failed: {}", e)))?;

        Ok(res.entries)
    }

    /// Persist distilled wisdom into Working Memory with basic metadata and tags
    fn persist_wisdom_to_working_memory(
        &self,
        task: &Task,
        wisdom: &DistilledWisdom,
    ) -> Result<(), RuntimeError> {
        // Basic approx token estimate from content length
        let approx_tokens = wisdom.content.len() / 4;

        // Create deterministic id seed from task name + current time (seconds)
        let id = format!("wm-{}-{}", task.name, chrono::Utc::now().timestamp());

        // Tags for retrieval (OR semantics in backend)
        let tags = [
            "wisdom".to_string(),
            "causal-chain".to_string(),
            "distillation".to_string(),
        ];

        // Build entry with new types API; use current time as timestamp
        let timestamp_s = chrono::Utc::now().timestamp() as u64;
        let entry = WorkingMemoryEntry {
            id,
            title: format!("Distilled wisdom for {}", task.name),
            content: wisdom.content.clone(),
            tags: tags.into_iter().collect(),
            timestamp_s,
            approx_tokens,
            meta: crate::ccos::working_memory::types::WorkingMemoryMeta {
                action_id: None,
                plan_id: None,
                intent_id: None,
                step_id: None,
                provider: Some("context-horizon".to_string()),
                attestation_hash: None,
                content_hash: None,
                extra: {
                    let mut m = HashMap::new();
                    m.insert("source".into(), "causal_chain_distillation".into());
                    m.insert("task".into(), task.name.clone());
                    m
                },
            },
        };

        let mut wm = self
            .working_memory
            .lock()
            .map_err(|_| RuntimeError::Generic("WorkingMemory lock poisoned".into()))?;
        wm.append(entry)
            .map_err(|e| RuntimeError::Generic(format!("WorkingMemory append failed: {}", e)))
    }

    /// Get current configuration
    pub fn get_config(&self) -> &ContextHorizonConfig {
        &self.config
    }
}

/// Configuration for context horizon management
#[derive(Debug, Clone)]
pub struct ContextHorizonConfig {
    pub max_tokens: usize,
    pub max_intent_tokens: usize,
    pub max_wisdom_tokens: usize,
    pub max_plan_tokens: usize,
    pub max_intents: usize,
    pub enable_semantic_search: bool,
    pub enable_wisdom_distillation: bool,
    pub enable_plan_abstraction: bool,
}

impl Default for ContextHorizonConfig {
    fn default() -> Self {
        Self {
            max_tokens: 8000,        // Conservative token limit
            max_intent_tokens: 4000, // 50% for intents
            max_wisdom_tokens: 2000, // 25% for wisdom
            max_plan_tokens: 2000,   // 25% for plan
            max_intents: 50,         // Reasonable intent limit
            enable_semantic_search: true,
            enable_wisdom_distillation: true,
            enable_plan_abstraction: true,
        }
    }
}

/// Intent Graph Virtualization for context horizon management
pub struct IntentGraphVirtualization {
    semantic_search: SemanticSearchEngine,
    graph_traversal: GraphTraversalEngine,
}

impl IntentGraphVirtualization {
    pub fn new() -> Self {
        Self {
            semantic_search: SemanticSearchEngine::new(),
            graph_traversal: GraphTraversalEngine::new(),
        }
    }

    pub fn find_relevant_intents(&self, task: &Task) -> Result<Vec<Intent>, RuntimeError> {
        // Extract search query from task
        let query = self.extract_search_query(task);

        // Use semantic search to find relevant intents
        let relevant_ids = self.semantic_search.search(&query)?;

        // Load intents from storage (placeholder - would use actual IntentGraph)
        let mut intents = Vec::new();
        for intent_id in relevant_ids {
            // In a real implementation, this would query the IntentGraph
            let intent = Intent::new(format!("Intent for {}", intent_id));
            intents.push(intent);
        }

        Ok(intents)
    }

    fn extract_search_query(&self, task: &Task) -> String {
        // Extract meaningful search terms from task
        // This is a simplified implementation
        format!("task:{}", task.name)
    }
}

/// Semantic search engine (placeholder implementation)
pub struct SemanticSearchEngine;

impl SemanticSearchEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn search(&self, query: &str) -> Result<Vec<IntentId>, RuntimeError> {
        // Placeholder implementation
        // In a real implementation, this would use vector embeddings
        Ok(vec![format!("intent-{}", query)])
    }
}

/// Graph traversal engine (placeholder implementation)
pub struct GraphTraversalEngine;

impl GraphTraversalEngine {
    pub fn new() -> Self {
        Self
    }
}

/// Causal Chain Distillation for context horizon management
pub struct CausalChainDistillation {
    ledger_analyzer: LedgerAnalyzer,
    pattern_recognizer: PatternRecognizer,
    wisdom_distiller: WisdomDistiller,
}

impl CausalChainDistillation {
    pub fn new() -> Self {
        Self {
            ledger_analyzer: LedgerAnalyzer::new(),
            pattern_recognizer: PatternRecognizer::new(),
            wisdom_distiller: WisdomDistiller::new(),
        }
    }

    pub fn get_distilled_wisdom(&self) -> Result<DistilledWisdom, RuntimeError> {
        // Analyze complete causal chain ledger
        let patterns = self.pattern_recognizer.find_patterns()?;
        let insights = self.ledger_analyzer.generate_insights()?;

        // Distill into low-token summaries
        let wisdom = self.wisdom_distiller.distill(patterns, insights)?;

        Ok(wisdom)
    }
}

/// Ledger analyzer for causal chain analysis
pub struct LedgerAnalyzer;

impl LedgerAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn generate_insights(&self) -> Result<Vec<String>, RuntimeError> {
        // Placeholder implementation
        // In a real implementation, this would analyze the causal chain
        Ok(vec!["Insight 1".to_string(), "Insight 2".to_string()])
    }
}

/// Pattern recognizer for causal chain analysis
pub struct PatternRecognizer;

impl PatternRecognizer {
    pub fn new() -> Self {
        Self
    }

    pub fn find_patterns(&self) -> Result<Vec<String>, RuntimeError> {
        // Placeholder implementation
        // In a real implementation, this would identify patterns in the causal chain
        Ok(vec!["Pattern 1".to_string(), "Pattern 2".to_string()])
    }
}

/// Wisdom distiller for creating low-token summaries
pub struct WisdomDistiller;

impl WisdomDistiller {
    pub fn new() -> Self {
        Self
    }

    pub fn distill(
        &self,
        patterns: Vec<String>,
        insights: Vec<String>,
    ) -> Result<DistilledWisdom, RuntimeError> {
        let mut wisdom = DistilledWisdom::new();

        // Convert patterns to failure patterns
        wisdom.content = format!("patterns: {:?}, insights: {:?}", patterns, insights);

        // Add placeholder data for other fields
        wisdom
            // Placeholder for agent reliability scores
            .content
            .push_str("agent_reliability_score");
        // Placeholder for cost insights
        wisdom.content.push_str("avg_cost");
        // Placeholder for performance metrics
        wisdom.content.push_str("avg_duration");

        Ok(wisdom)
    }
}

/// Plan Abstraction for context horizon management
pub struct PlanAbstraction {
    hierarchical_plans: HierarchicalPlanBuilder,
    data_handles: DataHandleManager,
    streaming: StreamingDataProcessor,
}

impl PlanAbstraction {
    pub fn new() -> Self {
        Self {
            hierarchical_plans: HierarchicalPlanBuilder::new(),
            data_handles: DataHandleManager::new(),
            streaming: StreamingDataProcessor::new(),
        }
    }

    pub fn create_abstract_plan(&self, task: &Task) -> Result<AbstractPlan, RuntimeError> {
        // Convert concrete plan to abstract references
        let abstract_steps = self.hierarchical_plans.create_abstract_steps(task)?;
        let data_handles = self.data_handles.create_data_handles(task)?;

        let mut plan = AbstractPlan::new();
        plan.steps = abstract_steps.into_iter().map(|step| step.name).collect();
        plan.data_handles = data_handles;

        Ok(plan)
    }
}

/// Hierarchical plan builder
pub struct HierarchicalPlanBuilder;

impl HierarchicalPlanBuilder {
    pub fn new() -> Self {
        Self
    }

    pub fn create_abstract_steps(&self, task: &Task) -> Result<Vec<AbstractStep>, RuntimeError> {
        // Placeholder implementation
        // In a real implementation, this would convert concrete plan steps to abstract ones
        Ok(vec![AbstractStep {
            name: "abstract_function".to_string(),
        }])
    }
}

/// Data handle manager
pub struct DataHandleManager;

impl DataHandleManager {
    pub fn new() -> Self {
        Self
    }

    pub fn create_data_handles(&self, task: &Task) -> Result<Vec<ResourceId>, RuntimeError> {
        // Placeholder implementation
        // In a real implementation, this would identify and create handles for large data
        Ok(vec!["resource-1".to_string()])
    }
}

/// Streaming data processor
pub struct StreamingDataProcessor;

impl StreamingDataProcessor {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_horizon_manager_creation() {
        let manager = ContextHorizonManager::new();
        assert!(manager.is_ok());

        // Check Working Memory is present and usable
        let wm = manager.unwrap().working_memory();
        let guard = wm.lock();
        assert!(guard.is_ok());
    }

    #[test]
    fn test_context_constraints() {
        let manager = ContextHorizonManager::new().unwrap();
        let config = ContextHorizonConfig::default();

        assert_eq!(config.max_tokens, 8000);
        assert_eq!(config.max_intents, 50);
    }

    #[test]
    fn test_token_estimation() {
        let manager = ContextHorizonManager::new().unwrap();
        let intents = vec![
            Intent::new("Test goal 1".to_string()),
            Intent::new("Test goal 2".to_string()),
            Intent::new("Test goal 3".to_string()),
        ];

        let tokens = manager.estimate_intent_tokens(&intents);
        assert!(tokens > 0);
    }

    #[test]
    fn test_context_reduction() {
        let manager = ContextHorizonManager::new().unwrap();
        let intents = vec![
            Intent::new("Test goal 1".to_string()),
            Intent::new("Test goal 2".to_string()),
            Intent::new("Test goal 3".to_string()),
        ];

        let reduced = manager.reduce_intents(intents, 100).unwrap();
        assert!(reduced.len() <= 3);
    }

    #[test]
    fn test_persist_wisdom_to_working_memory() {
        let manager = ContextHorizonManager::new().unwrap();
        let task = Task::new("unit-test".to_string(), "desc".to_string());
        let wisdom = DistilledWisdom {
            content: "some compact summary".to_string(),
        };

        // Should append an entry without error
        manager
            .persist_wisdom_to_working_memory(&task, &wisdom)
            .unwrap();

        let wm = manager.working_memory();
        let guard = wm.lock().unwrap();
        use crate::ccos::working_memory::backend::QueryParams;
        let results = guard
            .query(&QueryParams::with_tags(["wisdom"]).with_limit(Some(10)))
            .unwrap();
        assert!(!results.entries.is_empty());
    }

    #[test]
    fn test_boundary_to_query_params_time_and_token() {
        let manager = ContextHorizonManager::new().unwrap();

        use crate::ccos::working_memory::boundaries::{Boundary, BoundaryType};

        // TimeLimit
        let b_time = Boundary::new("time", BoundaryType::TimeLimit)
            .with_constraint("from_ts", serde_json::json!(100u64))
            .with_constraint("to_ts", serde_json::json!(200u64));
        let qp_time = manager.boundary_to_query_params(&b_time);
        assert_eq!(qp_time.from_ts_s, Some(100));
        assert_eq!(qp_time.to_ts_s, Some(200));

        // TokenLimit
        let b_token = Boundary::new("token", BoundaryType::TokenLimit)
            .with_constraint("max_tokens", serde_json::json!(5usize));
        let qp_token = manager.boundary_to_query_params(&b_token);
        assert_eq!(qp_token.limit, Some(5));
    }

    #[test]
    fn test_fetch_wisdom_from_working_memory_time_window() {
        let manager = ContextHorizonManager::new().unwrap();

        // Seed WM with a few entries at different timestamps
        let task = Task::new("wm-boundary-test".to_string(), "desc".to_string());
        let w1 = DistilledWisdom {
            content: "w1".into(),
        };
        let w2 = DistilledWisdom {
            content: "w2".into(),
        };
        let w3 = DistilledWisdom {
            content: "w3".into(),
        };

        // Inject entries with controlled timestamps by temporarily appending directly
        // through WorkingMemory facade after building entries via persist helper but adjusting timestamps.
        manager
            .persist_wisdom_to_working_memory(&task, &w1)
            .unwrap();
        manager
            .persist_wisdom_to_working_memory(&task, &w2)
            .unwrap();
        manager
            .persist_wisdom_to_working_memory(&task, &w3)
            .unwrap();

        // Define a time window using current time bounds to likely include all
        let now = chrono::Utc::now().timestamp() as u64;
        let from_ts = now.saturating_sub(60);
        let to_ts = now.saturating_add(60);

        use crate::ccos::working_memory::boundaries::{Boundary, BoundaryType};
        let boundaries = vec![
            Boundary::new("time", BoundaryType::TimeLimit)
                .with_constraint("from_ts", serde_json::json!(from_ts))
                .with_constraint("to_ts", serde_json::json!(to_ts)),
            Boundary::new("limit", BoundaryType::TokenLimit)
                .with_constraint("max_tokens", serde_json::json!(10usize)),
        ];

        let entries = manager
            .fetch_wisdom_from_working_memory(&boundaries)
            .unwrap();
        assert!(!entries.is_empty());
    }
}
