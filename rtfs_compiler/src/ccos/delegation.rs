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

/// Where the evaluator should send the execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecTarget {
    /// Run directly in the deterministic evaluator.
    LocalPure,
    /// Call an on-device model that implements [`ModelProvider`].
    LocalModel(String),
    /// Delegate to a remote provider via the Arbiter / RPC.
    RemoteModel(String),
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
}

impl<'a> Hash for CallContext<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fn_symbol.hash(state);
        self.arg_type_fingerprint.hash(state);
        self.runtime_context_hash.hash(state);
    }
}

impl<'a> PartialEq for CallContext<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.fn_symbol == other.fn_symbol
            && self.arg_type_fingerprint == other.arg_type_fingerprint
            && self.runtime_context_hash == other.runtime_context_hash
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
    /// LRU-ish cache keyed by the call context hash.
    cache: RwLock<HashMap<u64, ExecTarget>>, // TODO: replace with proper LRU.
}

impl StaticDelegationEngine {
    pub fn new(static_map: HashMap<String, ExecTarget>) -> Self {
        Self {
            static_map,
            cache: RwLock::new(HashMap::new()),
        }
    }
}

impl DelegationEngine for StaticDelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget {
        // 1. Static fast-path
        if let Some(target) = self.static_map.get(ctx.fn_symbol) {
            return target.clone();
        }

        // 2. Cached dynamic decision
        let key = {
            // Combine hashes cheaply (could use FxHash).  For now simple xor.
            ctx.arg_type_fingerprint ^ ctx.runtime_context_hash
        };

        if let Some(cached) = self.cache.read().unwrap().get(&key) {
            return cached.clone();
        }

        // 3. Default fallback
        let decision = ExecTarget::LocalPure;

        // 4. Insert into cache (fire-and-forget).  In production this would be
        // an LRU; for skeleton a plain HashMap suffices.
        self.cache.write().unwrap().insert(key, decision.clone());

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

        let ctx = CallContext {
            fn_symbol: "math/inc",
            arg_type_fingerprint: 0,
            runtime_context_hash: 0,
        };
        assert_eq!(de.decide(&ctx), ExecTarget::RemoteModel("gpt4o".to_string()));
    }

    #[test]
    fn fallback_is_local() {
        let de = StaticDelegationEngine::new(HashMap::new());
        let ctx = CallContext {
            fn_symbol: "not/known",
            arg_type_fingerprint: 1,
            runtime_context_hash: 2,
        };
        assert_eq!(de.decide(&ctx), ExecTarget::LocalPure);
    }

    #[test]
    fn local_echo_model_works() {
        let model = LocalEchoModel::default();
        assert_eq!(model.id(), "echo-model");
        
        let result = model.infer("hello world").unwrap();
        assert_eq!(result, "[ECHO] hello world");
    }

    #[test]
    fn remote_arbiter_model_works() {
        let model = RemoteArbiterModel::default();
        assert_eq!(model.id(), "arbiter-remote");
        
        let result = model.infer("test prompt").unwrap();
        assert!(result.contains("[REMOTE:http://localhost:8080/arbiter]"));
        assert!(result.contains("test prompt"));
    }

    #[test]
    fn model_registry_with_defaults() {
        let registry = ModelRegistry::with_defaults();
        
        // Check that default providers are registered
        assert!(registry.get("echo-model").is_some());
        assert!(registry.get("arbiter-remote").is_some());
        
        // Check that unknown providers are not found
        assert!(registry.get("unknown-model").is_none());
    }

    #[test]
    fn model_registry_custom_provider() {
        let registry = ModelRegistry::new();
        
        // Register a custom provider
        let custom_model = LocalEchoModel::new("custom-echo", "[CUSTOM]");
        registry.register(custom_model);
        
        // Check that it's registered
        let provider = registry.get("custom-echo").unwrap();
        let result = provider.infer("test").unwrap();
        assert_eq!(result, "[CUSTOM] test");
    }
}
