//! Core types for the hint handler framework.

use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::RuntimeResult;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::capability_marketplace::CapabilityMarketplace;
use crate::causal_chain::CausalChain;
use std::sync::Mutex;

/// Boxed future type for async handler methods.
/// Note: Not Send because the underlying marketplace execution isn't Send.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Type alias for the "next" executor in the handler chain.
/// Each handler calls this to execute the wrapped handlers/base execution.
pub type NextExecutor<'a> = Box<dyn FnOnce() -> BoxFuture<'a, RuntimeResult<Value>> + 'a>;

/// Context passed to hint handlers for execution.
/// Provides access to the capability marketplace and causal chain for logging.
pub struct ExecutionContext {
    pub capability_marketplace: Arc<CapabilityMarketplace>,
    pub causal_chain: Arc<Mutex<CausalChain>>,
}

impl ExecutionContext {
    pub fn new(
        capability_marketplace: Arc<CapabilityMarketplace>,
        causal_chain: Arc<Mutex<CausalChain>>,
    ) -> Self {
        Self {
            capability_marketplace,
            causal_chain,
        }
    }
}

/// Trait for modular execution hint handlers.
///
/// Each hint handler implements a specific behavior (retry, timeout, fallback, etc.).
/// Handlers are registered with the HintHandlerRegistry and applied based on their priority.
///
/// # Priority Guidelines
/// - 0-9: Pre-execution (logging, tracing)
/// - 10-19: Resilience (retry, circuit breaker)
/// - 20-29: Resource limits (timeout, rate limit)
/// - 30-39: Error handling (fallback, default values)
/// - 40+: Post-execution (caching, metrics)
///
/// # Handler Chaining
/// Handlers wrap each other based on priority. Lower priority runs first (outer wrapper).
/// Each handler receives a `next` function to call the inner handlers.
///
/// Example execution order for Retry(10) + Timeout(20) + Fallback(30):
/// ```text
/// Retry {
///     Timeout {
///         base_execution()
///     }
///     if failed -> Fallback { ... }
/// }
/// ```
pub trait HintHandler: Send + Sync {
    /// The unique key for this handler (e.g., "runtime.learning.retry").
    /// This must match the key used in RTFS metadata.
    fn hint_key(&self) -> &str;

    /// Priority for chaining. Lower values run first (wrap outer).
    /// If two handlers have the same priority, registration order is used.
    fn priority(&self) -> u32;

    /// Optional: Validate hint value format.
    /// Returns Ok(()) if valid, Err with message if invalid.
    fn validate_hint(&self, _hint_value: &Value) -> RuntimeResult<()> {
        Ok(())
    }

    /// Apply this handler's logic, wrapping the `next` executor.
    ///
    /// The handler should:
    /// 1. Do any pre-execution work (logging, setup)
    /// 2. Call `next()` to execute wrapped handlers / base execution
    /// 3. Do any post-execution work (error handling, retries)
    ///
    /// # Arguments
    /// * `host_call` - The capability call being executed
    /// * `hint_value` - The value from the hint metadata (e.g., `{:max-retries 3}`)
    /// * `ctx` - Execution context with marketplace and causal chain
    /// * `next` - Function to call the next handler in the chain (or base execution)
    fn apply<'a>(
        &'a self,
        host_call: &'a HostCall,
        hint_value: &'a Value,
        ctx: &'a ExecutionContext,
        next: NextExecutor<'a>,
    ) -> BoxFuture<'a, RuntimeResult<Value>>;
}

/// Arc-wrapped hint handler for use in the registry.
pub type ArcHintHandler = Arc<dyn HintHandler>;
