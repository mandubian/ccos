//! Built-in hint handlers for execution middleware.
//!
//! Handlers are applied in priority order (lowest first):
//! - metrics (1): Timing and call statistics
//! - cache (2): Memoization of results
//! - circuit_breaker (3): Failure protection
//! - rate_limit (5): Throttling
//! - retry (10): Exponential backoff
//! - timeout (20): Time limits
//! - fallback (30): Alternative on error

mod cache;
mod circuit_breaker;
mod fallback;
mod metrics;
mod rate_limit;
mod retry;
mod timeout;

pub use cache::CacheHintHandler;
pub use circuit_breaker::CircuitBreakerHintHandler;
pub use fallback::FallbackHintHandler;
pub use metrics::MetricsHintHandler;
pub use rate_limit::RateLimitHintHandler;
pub use retry::RetryHintHandler;
pub use timeout::TimeoutHintHandler;
