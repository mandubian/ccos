//! Delegation Engine (DE)
//!
//! Responsible for deciding whether a given RTFS function call should be
//! executed locally (pure evaluator), through a local model, or delegated to a
//! remote Arbiter / model provider.  This is a *skeleton* implementation: logic
//! is intentionally minimal but the public API is considered stable so that
//! the rest of the runtime can compile against it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};

use crate::ccos::caching::CacheStats;
use crate::ccos::caching::l1_delegation::{L1DelegationCache, DelegationPlan};

/// Where the evaluator should send the execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecTarget {
    /// Run directly in the deterministic evaluator.
    LocalPure,
    /// Call an on-device model that implements [`ModelProvider`].
    LocalModel(String),
    /// Delegate to a remote provider via the Arbiter / RPC.
    RemoteModel(String),
    /// Execute a pre-compiled RTFS module from the L4 content-addressable cache.
    L4CacheHit {
        /// Pointer to the bytecode in blob storage (e.g., S3 object key).
        storage_pointer: String,
        /// Cryptographic signature of the bytecode for verification.
        signature: String,
    },
}

/// Delegation metadata provided by CCOS components (intent graph, planners, etc.)
#[derive(Debug, Clone, Default)]
pub struct DelegationMetadata {
    /// Confidence score from the component that provided this metadata (0.0 - 1.0)
    pub confidence: Option<f64>,
    /// Human-readable reasoning from the component
    pub reasoning: Option<String>,
    /// Additional context from the intent graph or planning phase
    pub context: HashMap<String, String>,
    /// Component that provided this metadata (e.g., "intent-analyzer", "planner")
    pub source: Option<String>,
}

impl DelegationMetadata {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }
    
    pub fn with_reasoning(mut self, reasoning: String) -> Self {
        self.reasoning = Some(reasoning);
        self
    }
    
    pub fn with_context(mut self, key: String, value: String) -> Self {
        self.context.insert(key, value);
        self
    }
    
    pub fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }
}

/// Minimal call-site fingerprint used by the Delegation Engine.
#[derive(Debug, Clone)]
pub struct CallContext<'a> {
    /// Fully-qualified RTFS symbol name being invoked.
    pub fn_symbol: &'a str,
    /// Cheap structural hash of argument type information.
    pub arg_type_fingerprint: u64,
    /// Hash representing ambient runtime context (permissions, task, etc.).
    pub runtime_context_hash: u64,
    /// Optional semantic embedding of the original task description.
    /// This is used by the L4 cache for content-addressable lookups.
    pub semantic_hash: Option<Vec<f32>>,
    /// Optional delegation metadata from CCOS components
    pub metadata: Option<DelegationMetadata>,
}

impl<'a> CallContext<'a> {
    pub fn new(fn_symbol: &'a str, arg_type_fingerprint: u64, runtime_context_hash: u64) -> Self {
        Self {
            fn_symbol,
            arg_type_fingerprint,
            runtime_context_hash,
            semantic_hash: None,
            metadata: None,
        }
    }
    
    pub fn with_metadata(mut self, metadata: DelegationMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_semantic_hash(mut self, hash: Vec<f32>) -> Self {
        self.semantic_hash = Some(hash);
        self
    }
}

impl<'a> Hash for CallContext<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fn_symbol.hash(state);
        self.arg_type_fingerprint.hash(state);
        self.runtime_context_hash.hash(state);
        // Note: metadata and semantic_hash are not included in hash to maintain
        // consistency for caches (like L1) that key on CallContext structure.
        // L4 cache performs its own semantic search.
    }
}

impl<'a> PartialEq for CallContext<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.fn_symbol == other.fn_symbol
            && self.arg_type_fingerprint == other.arg_type_fingerprint
            && self.runtime_context_hash == other.runtime_context_hash
        // Note: metadata and semantic_hash are not compared to maintain cache consistency.
    }
}

impl<'a> Eq for CallContext<'a> {}

/// Trait implemented by any Delegation Engine.
///
/// Guarantee: *pure* – `decide` must be free of side-effects so that the
/// evaluator can safely cache the result.
pub trait DelegationEngine: Send + Sync + std::fmt::Debug {
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}

/// Simple static mapping + cache implementation.
#[derive(Debug)]
pub struct StaticDelegationEngine {
    /// Fast lookup for explicit per-symbol policies.
    static_map: HashMap<String, ExecTarget>,
    /// L1 Delegation Cache for (Agent, Task) -> Plan memoization
    pub l1_cache: Arc<L1DelegationCache>,
}

impl StaticDelegationEngine {
    pub fn new(static_map: HashMap<String, ExecTarget>) -> Self {
        Self {
            static_map,
            l1_cache: Arc::new(L1DelegationCache::with_default_config()),
        }
    }
    
    pub fn with_l1_cache(static_map: HashMap<String, ExecTarget>, l1_cache: Arc<L1DelegationCache>) -> Self {
        Self {
            static_map,
            l1_cache,
        }
    }
    
    /// Manually cache a delegation decision for future use
    pub fn cache_decision(&self, agent: &str, task: &str, target: ExecTarget, confidence: f64, reasoning: &str) {
        let plan = DelegationPlan::new(
            match target {
                ExecTarget::LocalPure => "local-pure".to_string(),
                ExecTarget::LocalModel(ref model) => format!("local-{}", model),
                ExecTarget::RemoteModel(ref model) => format!("remote-{}", model),
                // L4 hits are not cached at L1; they are a distinct path.
                ExecTarget::L4CacheHit { .. } => return,
            },
            confidence,
            reasoning.to_string(),
        );
        let _ = self.l1_cache.put_plan(agent, task, plan);
    }
    
    /// Cache a delegation decision with metadata from CCOS components
    pub fn cache_decision_with_metadata(&self, agent: &str, task: &str, target: ExecTarget, metadata: &DelegationMetadata) {
        let confidence = metadata.confidence.unwrap_or(0.8);
        let reasoning = metadata.reasoning.clone().unwrap_or_else(|| {
            format!("Decision from {}", metadata.source.as_deref().unwrap_or("unknown-component"))
        });
        
        let mut plan = DelegationPlan::new(
            match target {
                ExecTarget::LocalPure => "local-pure".to_string(),
                ExecTarget::LocalModel(ref model) => format!("local-{}", model),
                ExecTarget::RemoteModel(ref model) => format!("remote-{}", model),
                // L4 hits are not cached at L1.
                ExecTarget::L4CacheHit { .. } => return,
            },
            confidence,
            reasoning,
        );
        
        // Add context metadata to the plan
        for (key, value) in &metadata.context {
            plan = plan.with_metadata(key.clone(), value.clone());
        }
        
        let _ = self.l1_cache.put_plan(agent, task, plan);
    }
    
    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        self.l1_cache.get_stats()
    }
}

impl DelegationEngine for StaticDelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget {
        // 1. Static fast-path
        if let Some(target) = self.static_map.get(ctx.fn_symbol) {
            return target.clone();
        }

        // 2. L1 Cache lookup for delegation plan
        let agent = ctx.fn_symbol;
        let task = format!("{:x}", ctx.arg_type_fingerprint ^ ctx.runtime_context_hash);
        
        if let Some(plan) = self.l1_cache.get_plan(agent, &task) {
            // Convert plan target to ExecTarget
            match plan.target.as_str() {
                "local-pure" => return ExecTarget::LocalPure,
                target if target.starts_with("local-") => {
                    return ExecTarget::LocalModel(target.trim_start_matches("local-").to_string());
                }
                target if target.starts_with("remote-") => {
                    return ExecTarget::RemoteModel(target.trim_start_matches("remote-").to_string());
                }
                _ => {
                    // Fall through to default decision
                }
            }
        }

        // 3. Use metadata if available, otherwise default fallback
        let decision = ExecTarget::LocalPure;
        let confidence: f64;
        let reasoning: String;
        
        if let Some(ref metadata) = ctx.metadata {
            // Use confidence and reasoning from CCOS components
            confidence = metadata.confidence.unwrap_or(0.8);
            reasoning = metadata.reasoning.clone().unwrap_or_else(|| {
                format!("Decision from {}", metadata.source.as_deref().unwrap_or("unknown-component"))
            });
            
            // Cache with metadata
            self.cache_decision_with_metadata(agent, &task, decision.clone(), metadata);
        } else {
            // Default fallback
            confidence = 0.8;
            reasoning = "Default fallback to local pure execution".to_string();
            
            // Cache the decision as a delegation plan
            let plan = DelegationPlan::new(
                "local-pure".to_string(),
                confidence,
                reasoning,
            );
            let _ = self.l1_cache.put_plan(agent, &task, plan);
        }

        decision
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Model Provider abstraction
// ──────────────────────────────────────────────────────────────────────────

/// Anything capable of transforming a textual prompt into a textual output.
/// A provider may be a quantized on-device transformer, a regex rule engine,
/// or a remote OpenAI deployment – the Delegation Engine does not care.
pub trait ModelProvider: Send + Sync + std::fmt::Debug {
    /// Stable identifier (e.g. "phi-mini", "gpt4o").
    fn id(&self) -> &'static str;
    /// Perform inference.  Blocking call for now; async wrapper lives above.
    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

/// Simple echo model for testing - just returns the input with a prefix
#[derive(Debug)]
pub struct LocalEchoModel {
    id: &'static str,
    prefix: String,
}

impl LocalEchoModel {
    pub fn new(id: &'static str, prefix: &str) -> Self {
        Self {
            id,
            prefix: prefix.to_string(),
        }
    }

    pub fn default() -> Self {
        Self::new("echo-model", "[ECHO]")
    }
}

impl ModelProvider for LocalEchoModel {
    fn id(&self) -> &'static str {
        self.id
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok(format!("{} {}", self.prefix, prompt))
    }
}

/// Remote model stub that would delegate to Arbiter RPC
#[derive(Debug)]
pub struct RemoteArbiterModel {
    id: &'static str,
    endpoint_url: String,
}

impl RemoteArbiterModel {
    pub fn new(id: &'static str, endpoint_url: &str) -> Self {
        Self {
            id,
            endpoint_url: endpoint_url.to_string(),
        }
    }

    pub fn default() -> Self {
        Self::new("arbiter-remote", "http://localhost:8080/arbiter")
    }
}

impl ModelProvider for RemoteArbiterModel {
    fn id(&self) -> &'static str {
        self.id
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Implement actual RPC call to Arbiter
        // For now, return a stub response indicating remote delegation
        Ok(format!("[REMOTE:{}] {}", self.endpoint_url, prompt))
    }
}

/// Global registry.  In real code this might live elsewhere; kept here for
/// convenience while the API stabilises.
#[derive(Default, Debug)]
pub struct ModelRegistry {
    providers: RwLock<HashMap<String, Arc<dyn ModelProvider>>>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<P: ModelProvider + 'static>(&self, provider: P) {
        self.providers
            .write()
            .unwrap()
            .insert(provider.id().to_string(), Arc::new(provider));
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ModelProvider>> {
        self.providers.read().unwrap().get(id).cloned()
    }

    /// Register default providers for testing
    pub fn with_defaults() -> Self {
        let registry = Self::new();
        registry.register(LocalEchoModel::default());
        registry.register(RemoteArbiterModel::default());
        
        // Try to register realistic local model if available
        if let Ok(model_path) = std::env::var("RTFS_LOCAL_MODEL_PATH") {
            if std::path::Path::new(&model_path).exists() {
                registry.register(crate::ccos::local_models::LocalLlamaModel::default());
            }
        }
        
        registry
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Tests (compile-time only)
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_map_override() {
        let mut map = HashMap::new();
        map.insert("math/inc".to_string(), ExecTarget::RemoteModel("gpt4o".to_string()));
        let de = StaticDelegationEngine::new(map);

        let ctx = CallContext::new("math/inc", 0, 0);
        assert_eq!(de.decide(&ctx), ExecTarget::RemoteModel("gpt4o".to_string()));
    }

    #[test]
    fn fallback_is_local() {
        let de = StaticDelegationEngine::new(HashMap::new());
        let ctx = CallContext::new("not/known", 1, 2);
        assert_eq!(de.decide(&ctx), ExecTarget::LocalPure);
    }

    #[test]
    fn l1_cache_integration_test() {
        let de = StaticDelegationEngine::new(HashMap::new());
        let ctx = CallContext::new("user/get_preferences", 123, 456);

        // 1. First call: miss
        assert_eq!(de.decide(&ctx), ExecTarget::LocalPure);
        assert_eq!(de.cache_stats().hits, 0);
        assert_eq!(de.cache_stats().misses, 1);

        // Manually cache a different decision for the same context
        let agent = "user/get_preferences";
        let task = format!("{:x}", 123u64 ^ 456u64);
        de.cache_decision(agent, &task, ExecTarget::LocalModel("fast-model".to_string()), 0.9, "test decision");

        // 2. Second call: hit
        assert_eq!(de.decide(&ctx), ExecTarget::LocalModel("fast-model".to_string()));
        assert_eq!(de.cache_stats().hits, 1);
    }

    #[test]
    fn task_xor_generation_test() {
        let ctx1 = CallContext::new("agent1", 12345, 67890);
        let ctx2 = CallContext::new("agent1", 12345, 67891); // different runtime context
        let ctx3 = CallContext::new("agent2", 12345, 67890); // different agent

        let task1 = format!("{:x}", ctx1.arg_type_fingerprint ^ ctx1.runtime_context_hash);
        let task2 = format!("{:x}", ctx2.arg_type_fingerprint ^ ctx2.runtime_context_hash);
        let task3 = format!("{:x}", ctx3.arg_type_fingerprint ^ ctx3.runtime_context_hash);

        assert_ne!(task1, task2);
        // agent is not part of task generation, so task1 and task3 should be the same
        assert_eq!(task1, task3);
    }

    #[test]
    fn cache_manual_operations_test() {
        let de = StaticDelegationEngine::new(HashMap::new());
        let agent = "manual/agent";
        let task = "manual_task";
        
        de.cache_decision(agent, task, ExecTarget::RemoteModel("test-model".to_string()), 0.99, "manual entry");
        
        let plan = de.l1_cache.get_plan(agent, task).unwrap();
        assert_eq!(plan.target, "remote-test-model");
        assert_eq!(plan.confidence, 0.99);
    }
    
    #[test]
    fn call_context_with_semantic_hash() {
        let ctx = CallContext::new("test/fn", 1, 2)
            .with_semantic_hash(vec![0.1, 0.2, 0.3]);
        
        assert_eq!(ctx.fn_symbol, "test/fn");
        assert!(ctx.semantic_hash.is_some());
        assert_eq!(ctx.semantic_hash.unwrap(), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn local_echo_model_works() {
        let model = LocalEchoModel::default();
        assert_eq!(model.id(), "echo-model");
        let result = model.infer("hello").unwrap();
        assert_eq!(result, "[ECHO] hello");
    }

    #[test]
    fn remote_arbiter_model_works() {
        let model = RemoteArbiterModel::default();
        assert_eq!(model.id(), "arbiter-remote");
        let result = model.infer("task-123");
        // Stub implementation always returns a formatted string.
        assert!(result.is_ok());
        let s = result.unwrap();
        assert!(s.contains("[REMOTE:"));
    }

    #[test]
    fn model_registry_with_defaults() {
        let registry = ModelRegistry::with_defaults();
        assert!(registry.get("echo-model").is_some());
        assert!(registry.get("arbiter-remote").is_some());
        assert!(registry.get("non-existent").is_none());
    }

    #[test]
    fn model_registry_custom_provider() {
        let registry = ModelRegistry::new();
        
        // Register a custom provider
        let custom_model = LocalEchoModel::new("custom-model", "[CUSTOM]");
        registry.register(custom_model);
        
        // Verify it's available
        assert!(registry.get("custom-model").is_some());
        
        // Test inference
        let provider = registry.get("custom-model").unwrap();
        let result = provider.infer("test input").unwrap();
        assert_eq!(result, "[CUSTOM] test input");
    }
    
    #[test]
    fn delegation_with_metadata() {
        // Test that the delegation engine can use metadata from CCOS components
        let de = StaticDelegationEngine::new(HashMap::new());
        
        // Create metadata from a hypothetical intent analyzer
        let metadata = DelegationMetadata::new()
            .with_confidence(0.95)
            .with_reasoning("Intent analysis suggests local execution for mathematical operations".to_string())
            .with_context("intent_type".to_string(), "mathematical".to_string())
            .with_context("complexity".to_string(), "low".to_string())
            .with_source("intent-analyzer".to_string());
        
        let ctx = CallContext::new("math/add", 0x12345678, 0xABCDEF01)
            .with_metadata(metadata.clone());
        
        // The decision should be cached with the provided metadata
        let result = de.decide(&ctx);
        assert_eq!(result, ExecTarget::LocalPure);
        
        // Verify the cache contains the metadata
        let agent = "math/add";
        let task = format!("{:x}", 0x12345678u64 ^ 0xABCDEF01u64);
        let plans = de.l1_cache.get_agent_plans(agent);
        assert!(!plans.is_empty());
        
        // Check that the plan has the expected metadata
        let (_, plan) = plans.first().unwrap();
        assert_eq!(plan.confidence, 0.95);
        assert!(plan.reasoning.contains("Intent analysis suggests"));
        assert_eq!(plan.metadata.get("intent_type"), Some(&"mathematical".to_string()));
        assert_eq!(plan.metadata.get("complexity"), Some(&"low".to_string()));
    }
}
