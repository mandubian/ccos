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
pub trait ModelProvider: Send + Sync {
    /// Stable identifier (e.g. "phi-mini", "gpt4o").
    fn id(&self) -> &'static str;
    /// Perform inference.  Blocking call for now; async wrapper lives above.
    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

/// Global registry.  In real code this might live elsewhere; kept here for
/// convenience while the API stabilises.
#[derive(Default)]
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
}
