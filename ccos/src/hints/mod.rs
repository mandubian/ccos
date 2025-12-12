//! Modular Execution Hint Handlers
//!
//! This module provides an extensible framework for applying execution hints
//! (retry, timeout, fallback, etc.) to capability calls. Instead of a monolithic
//! if-else chain, each hint type is implemented as a separate handler that can
//! be registered and composed dynamically.
//!
//! # Architecture
//!
//! - `HintHandler` trait: Defines the interface for hint handlers
//! - `HintHandlerRegistry`: Stores and chains handlers by priority
//! - Built-in handlers: `RetryHintHandler`, `TimeoutHintHandler`, `FallbackHintHandler`
//!
//! # Example
//!
//! ```ignore
//! let registry = HintHandlerRegistry::with_defaults();
//! registry.register(Arc::new(MyCustomHintHandler::new()));
//!
//! let result = registry.execute_with_hints(&host_call, &hints, executor).await;
//! ```

mod handlers;
mod registry;
mod types;

pub use handlers::{
    FallbackHintHandler, RateLimitHintHandler, RetryHintHandler, TimeoutHintHandler,
};
pub use registry::HintHandlerRegistry;
pub use types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
