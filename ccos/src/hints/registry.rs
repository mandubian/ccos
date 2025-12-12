//! HintHandlerRegistry - manages and chains hint handlers dynamically.

use super::types::{ArcHintHandler, BoxFuture, ExecutionContext, NextExecutor};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::RuntimeResult;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for execution hint handlers.
///
/// Handlers are stored and sorted by priority. When executing with hints,
/// the registry chains applicable handlers dynamically, creating a middleware
/// pipeline where each handler can wrap the next.
///
/// # Handler Priority
/// Lower priority handlers run first (wrap outer). For example:
/// - Retry (10) wraps Timeout (20) wraps Fallback (30) wraps base execution
///
/// # Execution Model
/// ```text
/// Retry.apply(next = |
///   Timeout.apply(next = |
///     Fallback.apply(next = |
///       marketplace.execute()
///     |)
///   |)
/// |)
/// ```
pub struct HintHandlerRegistry {
    /// Handlers sorted by priority (lowest first)
    handlers: Vec<ArcHintHandler>,
}

impl Default for HintHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandlerRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Creates a registry with the default built-in handlers.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        // Register in any order - they get sorted by priority
        registry.register(Arc::new(super::handlers::MetricsHintHandler::new()));
        registry.register(Arc::new(super::handlers::CacheHintHandler::new()));
        registry.register(Arc::new(super::handlers::CircuitBreakerHintHandler::new()));
        registry.register(Arc::new(super::handlers::RateLimitHintHandler::new()));
        registry.register(Arc::new(super::handlers::RetryHintHandler::new()));
        registry.register(Arc::new(super::handlers::TimeoutHintHandler::new()));
        registry.register(Arc::new(super::handlers::FallbackHintHandler::new()));
        registry
    }

    /// Registers a new handler. Handlers are automatically sorted by priority.
    pub fn register(&mut self, handler: ArcHintHandler) {
        self.handlers.push(handler);
        self.handlers.sort_by_key(|h| h.priority());
    }

    /// Unregisters a handler by its hint key.
    /// Returns true if a handler was removed.
    pub fn unregister(&mut self, hint_key: &str) -> bool {
        let before = self.handlers.len();
        self.handlers.retain(|h| h.hint_key() != hint_key);
        self.handlers.len() < before
    }

    /// Returns a list of all registered handler keys in priority order.
    pub fn list_handlers(&self) -> Vec<&str> {
        self.handlers.iter().map(|h| h.hint_key()).collect()
    }

    /// Returns handler metadata as (key, priority, description) tuples.
    pub fn handler_info(&self) -> Vec<(&str, u32, &str)> {
        self.handlers
            .iter()
            .map(|h| (h.hint_key(), h.priority(), h.description()))
            .collect()
    }

    /// Creates a new registry with only the specified handler types.
    /// Useful for creating minimal registries for testing or specific use cases.
    pub fn with_handlers<F>(builder: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        let mut registry = Self::new();
        builder(&mut registry);
        registry
    }

    /// Returns the number of registered handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Returns true if no handlers are registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Execute a capability call with the given hints.
    ///
    /// Builds a dynamic execution chain from applicable handlers, ordered by priority.
    /// Each handler's `apply` method wraps the next handler in the chain.
    ///
    /// # Handler Chaining
    /// Handlers are applied in priority order (lowest first = outermost wrapper).
    /// The innermost function is the base marketplace execution.
    pub async fn execute_with_hints(
        &self,
        host_call: &HostCall,
        hints: &HashMap<String, Value>,
        ctx: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Find applicable handlers (those whose key is in hints)
        let applicable: Vec<_> = self
            .handlers
            .iter()
            .filter(|h| hints.contains_key(h.hint_key()))
            .cloned()
            .collect();

        if applicable.is_empty() {
            // No applicable handlers, execute directly via marketplace
            return ctx
                .capability_marketplace
                .execute_capability_enhanced(
                    &host_call.capability_id,
                    &Value::List(host_call.args.clone()),
                    host_call.metadata.as_ref(),
                )
                .await;
        }

        // Build the execution chain dynamically
        // Reverse so we start from highest priority (innermost) and work outward
        let reversed: Vec<_> = applicable.into_iter().rev().collect();

        // Use a helper function to avoid self-reference issues
        execute_handler_chain(&reversed, 0, host_call, hints, ctx).await
    }
}

/// Execute the handler chain recursively (standalone function to avoid lifetime issues).
fn execute_handler_chain<'a>(
    handlers: &'a [ArcHintHandler],
    index: usize,
    host_call: &'a HostCall,
    hints: &'a HashMap<String, Value>,
    ctx: &'a ExecutionContext,
) -> BoxFuture<'a, RuntimeResult<Value>> {
    Box::pin(async move {
        if index >= handlers.len() {
            // Base case: no more handlers, execute via marketplace
            return ctx
                .capability_marketplace
                .execute_capability_enhanced(
                    &host_call.capability_id,
                    &Value::List(host_call.args.clone()),
                    host_call.metadata.as_ref(),
                )
                .await;
        }

        // Get current handler
        let handler = &handlers[index];
        let hint_value = hints
            .get(handler.hint_key())
            .expect("Handler should only be in list if hint exists");

        // Create the "next" executor that calls the rest of the chain
        let next: NextExecutor<'a> =
            Box::new(move || execute_handler_chain(handlers, index + 1, host_call, hints, ctx));

        // Apply this handler
        handler.apply(host_call, hint_value, ctx, next).await
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hints::types::HintHandler;

    /// Test handler with configurable priority for testing ordering
    struct TestHandler {
        key: String,
        priority: u32,
    }

    impl TestHandler {
        fn new(key: &str, priority: u32) -> Self {
            Self {
                key: key.to_string(),
                priority,
            }
        }
    }

    impl HintHandler for TestHandler {
        fn hint_key(&self) -> &str {
            &self.key
        }

        fn priority(&self) -> u32 {
            self.priority
        }

        fn apply<'a>(
            &'a self,
            _host_call: &'a HostCall,
            _hint_value: &'a Value,
            _ctx: &'a ExecutionContext,
            next: NextExecutor<'a>,
        ) -> BoxFuture<'a, RuntimeResult<Value>> {
            // Simple passthrough for testing
            Box::pin(async move { next().await })
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = HintHandlerRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        let registry = HintHandlerRegistry::with_defaults();
        assert_eq!(registry.len(), 3); // retry, timeout, fallback
        assert!(!registry.is_empty());
    }

    #[test]
    fn test_handler_priority_ordering() {
        let mut registry = HintHandlerRegistry::new();

        // Register handlers out of order
        registry.register(Arc::new(TestHandler::new("test.high", 100)));
        registry.register(Arc::new(TestHandler::new("test.low", 5)));
        registry.register(Arc::new(TestHandler::new("test.medium", 50)));

        // Verify they are sorted by priority (lowest first)
        assert_eq!(registry.handlers[0].hint_key(), "test.low");
        assert_eq!(registry.handlers[1].hint_key(), "test.medium");
        assert_eq!(registry.handlers[2].hint_key(), "test.high");
    }

    #[test]
    fn test_default_handlers_priority() {
        let registry = HintHandlerRegistry::with_defaults();

        // Verify built-in handler order: retry (10) < timeout (20) < fallback (30)
        assert_eq!(registry.handlers[0].hint_key(), "runtime.learning.retry");
        assert_eq!(registry.handlers[0].priority(), 10);

        assert_eq!(registry.handlers[1].hint_key(), "runtime.learning.timeout");
        assert_eq!(registry.handlers[1].priority(), 20);

        assert_eq!(registry.handlers[2].hint_key(), "runtime.learning.fallback");
        assert_eq!(registry.handlers[2].priority(), 30);
    }

    #[test]
    fn test_custom_handler_registration() {
        let mut registry = HintHandlerRegistry::with_defaults();
        assert_eq!(registry.len(), 3);

        // Add a custom handler with priority between timeout and fallback
        registry.register(Arc::new(TestHandler::new("custom.rate-limit", 25)));
        assert_eq!(registry.len(), 4);

        // Verify ordering is maintained
        assert_eq!(registry.handlers[0].hint_key(), "runtime.learning.retry"); // 10
        assert_eq!(registry.handlers[1].hint_key(), "runtime.learning.timeout"); // 20
        assert_eq!(registry.handlers[2].hint_key(), "custom.rate-limit"); // 25
        assert_eq!(registry.handlers[3].hint_key(), "runtime.learning.fallback");
        // 30
    }
}
