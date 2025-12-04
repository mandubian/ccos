//! Rate Limiting for MCP Discovery
//!
//! This module provides rate limiting and retry logic for MCP server requests.
//! It implements a token bucket algorithm for rate limiting and exponential
//! backoff for retries.
//!
//! ## Features
//!
//! - Token bucket rate limiting (configurable tokens per second)
//! - Per-server rate limits
//! - Exponential backoff retry with jitter
//! - Configurable retry policies
//! - Handles 429 (Too Many Requests) responses

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Rate limit configuration for MCP discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitConfig {
    /// Maximum requests per second per server
    pub requests_per_second: f64,
    /// Maximum burst size (tokens in bucket)
    pub burst_size: u32,
    /// Whether rate limiting is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10.0, // 10 requests per second
            burst_size: 20,            // Allow bursts of up to 20 requests
            enabled: true,
        }
    }
}

impl RateLimitConfig {
    /// Create a permissive rate limit config (for testing or trusted servers)
    pub fn permissive() -> Self {
        Self {
            requests_per_second: 100.0,
            burst_size: 100,
            enabled: true,
        }
    }

    /// Create a strict rate limit config (for rate-limited APIs)
    pub fn strict() -> Self {
        Self {
            requests_per_second: 1.0,
            burst_size: 5,
            enabled: true,
        }
    }

    /// Disable rate limiting entirely
    pub fn disabled() -> Self {
        Self {
            requests_per_second: f64::MAX,
            burst_size: u32::MAX,
            enabled: false,
        }
    }
}

/// Retry policy configuration (serializable version for config files)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetryPolicyConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay before first retry (in milliseconds)
    pub initial_delay_ms: u64,
    /// Maximum delay between retries (in milliseconds)
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles each time)
    pub exponential_base: f64,
    /// Whether to add jitter to retry delays
    pub jitter: bool,
}

impl Default for RetryPolicyConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            exponential_base: 2.0,
            jitter: true,
        }
    }
}

impl From<RetryPolicyConfig> for RetryPolicy {
    fn from(config: RetryPolicyConfig) -> Self {
        Self {
            max_retries: config.max_retries,
            initial_delay: Duration::from_millis(config.initial_delay_ms),
            max_delay: Duration::from_millis(config.max_delay_ms),
            backoff_multiplier: config.exponential_base,
            use_jitter: config.jitter,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
        }
    }
}

/// Retry policy configuration
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles each time)
    pub backoff_multiplier: f64,
    /// Whether to add jitter to retry delays
    pub use_jitter: bool,
    /// HTTP status codes that should trigger a retry
    pub retryable_status_codes: Vec<u16>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            use_jitter: true,
            retryable_status_codes: vec![
                429, // Too Many Requests
                500, // Internal Server Error
                502, // Bad Gateway
                503, // Service Unavailable
                504, // Gateway Timeout
            ],
        }
    }
}

impl RetryPolicy {
    /// Create a policy that never retries
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Create an aggressive retry policy for critical operations
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 1.5,
            use_jitter: true,
            retryable_status_codes: vec![429, 500, 502, 503, 504, 408],
        }
    }

    /// Check if a status code should trigger a retry
    pub fn should_retry_status(&self, status: u16) -> bool {
        self.retryable_status_codes.contains(&status)
    }

    /// Calculate delay for a given attempt number (0-indexed)
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let base_delay =
            self.initial_delay.as_secs_f64() * self.backoff_multiplier.powi((attempt - 1) as i32);

        let delay_secs = base_delay.min(self.max_delay.as_secs_f64());

        let final_delay = if self.use_jitter {
            // Add jitter: random value between 0.5x and 1.5x the delay
            let jitter_factor = 0.5 + rand_jitter() * 1.0;
            delay_secs * jitter_factor
        } else {
            delay_secs
        };

        Duration::from_secs_f64(final_delay)
    }
}

/// Simple pseudo-random jitter (0.0 to 1.0)
/// Uses a basic approach to avoid adding a dependency
fn rand_jitter() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos % 1000) as f64 / 1000.0
}

/// Token bucket for a single server
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_update: Instant,
    config: RateLimitConfig,
}

impl TokenBucket {
    fn new(config: RateLimitConfig) -> Self {
        Self {
            tokens: config.burst_size as f64,
            last_update: Instant::now(),
            config,
        }
    }

    /// Try to acquire a token, returns wait time if rate limited
    fn try_acquire(&mut self) -> Option<Duration> {
        if !self.config.enabled {
            return None; // No wait needed
        }

        self.refill();

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            None // No wait needed
        } else {
            // Calculate how long to wait for 1 token
            let tokens_needed = 1.0 - self.tokens;
            let wait_secs = tokens_needed / self.config.requests_per_second;
            Some(Duration::from_secs_f64(wait_secs))
        }
    }

    /// Refill tokens based on time elapsed
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);
        let new_tokens = elapsed.as_secs_f64() * self.config.requests_per_second;

        self.tokens = (self.tokens + new_tokens).min(self.config.burst_size as f64);
        self.last_update = now;
    }
}

/// Rate limiter that tracks rate limits per server endpoint
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    default_config: RateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter with default configuration
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            default_config: RateLimitConfig::default(),
        }
    }

    /// Create a rate limiter with custom default configuration
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            default_config: config,
        }
    }

    /// Acquire a token for the given server, waiting if necessary
    ///
    /// Returns Ok(()) when a request can be made, or Err with wait duration
    /// if the caller should wait.
    pub fn acquire(&self, server_endpoint: &str) -> Result<(), Duration> {
        let mut buckets = self.buckets.lock().unwrap();

        let bucket = buckets
            .entry(server_endpoint.to_string())
            .or_insert_with(|| TokenBucket::new(self.default_config.clone()));

        match bucket.try_acquire() {
            None => Ok(()),
            Some(wait_time) => Err(wait_time),
        }
    }

    /// Acquire a token, blocking asynchronously if necessary
    pub async fn acquire_async(&self, server_endpoint: &str) {
        loop {
            match self.acquire(server_endpoint) {
                Ok(()) => return,
                Err(wait_time) => {
                    log::debug!(
                        "Rate limited for {}, waiting {:?}",
                        server_endpoint,
                        wait_time
                    );
                    tokio::time::sleep(wait_time).await;
                }
            }
        }
    }

    /// Set a custom rate limit for a specific server
    pub fn set_server_config(&self, server_endpoint: &str, config: RateLimitConfig) {
        let mut buckets = self.buckets.lock().unwrap();
        buckets.insert(server_endpoint.to_string(), TokenBucket::new(config));
    }

    /// Clear rate limit state for a server
    pub fn clear_server(&self, server_endpoint: &str) {
        let mut buckets = self.buckets.lock().unwrap();
        buckets.remove(server_endpoint);
    }

    /// Clear all rate limit state
    pub fn clear_all(&self) {
        let mut buckets = self.buckets.lock().unwrap();
        buckets.clear();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Retry context for tracking retry attempts
#[derive(Debug, Clone)]
pub struct RetryContext {
    pub attempt: u32,
    pub policy: RetryPolicy,
    pub last_error: Option<String>,
}

impl RetryContext {
    pub fn new(policy: RetryPolicy) -> Self {
        Self {
            attempt: 0,
            policy,
            last_error: None,
        }
    }

    /// Check if another retry is allowed
    pub fn can_retry(&self) -> bool {
        self.attempt < self.policy.max_retries
    }

    /// Record an attempt and get the delay before the next retry
    pub fn next_attempt(&mut self, error: Option<String>) -> Option<Duration> {
        if !self.can_retry() {
            return None;
        }

        self.attempt += 1;
        self.last_error = error;
        Some(self.policy.delay_for_attempt(self.attempt))
    }

    /// Record a successful attempt (resets the context)
    pub fn success(&mut self) {
        self.attempt = 0;
        self.last_error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_second, 10.0);
        assert_eq!(config.burst_size, 20);
        assert!(config.enabled);
    }

    #[test]
    fn test_rate_limit_config_disabled() {
        let config = RateLimitConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert!(policy.should_retry_status(429));
        assert!(policy.should_retry_status(503));
        assert!(!policy.should_retry_status(404));
    }

    #[test]
    fn test_retry_policy_delay_calculation() {
        let policy = RetryPolicy {
            initial_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(60),
            use_jitter: false,
            ..Default::default()
        };

        assert_eq!(policy.delay_for_attempt(0), Duration::ZERO);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(2));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(4));
    }

    #[test]
    fn test_retry_policy_max_delay() {
        let policy = RetryPolicy {
            initial_delay: Duration::from_secs(10),
            backoff_multiplier: 10.0,
            max_delay: Duration::from_secs(30),
            use_jitter: false,
            ..Default::default()
        };

        // 10 * 10^3 = 10000 seconds, but capped at 30
        assert_eq!(policy.delay_for_attempt(4), Duration::from_secs(30));
    }

    #[test]
    fn test_rate_limiter_acquire() {
        let limiter = RateLimiter::with_config(RateLimitConfig {
            requests_per_second: 1000.0, // Fast for testing
            burst_size: 5,
            enabled: true,
        });

        // First few requests should succeed immediately
        for _ in 0..5 {
            assert!(limiter.acquire("http://test").is_ok());
        }

        // After burst, should be rate limited
        let result = limiter.acquire("http://test");
        assert!(result.is_err());
    }

    #[test]
    fn test_rate_limiter_disabled() {
        let limiter = RateLimiter::with_config(RateLimitConfig::disabled());

        // All requests should succeed when disabled
        for _ in 0..100 {
            assert!(limiter.acquire("http://test").is_ok());
        }
    }

    #[test]
    fn test_retry_context() {
        let mut ctx = RetryContext::new(RetryPolicy {
            max_retries: 2,
            ..Default::default()
        });

        assert!(ctx.can_retry());
        assert!(ctx.next_attempt(Some("error 1".to_string())).is_some());
        assert!(ctx.can_retry());
        assert!(ctx.next_attempt(Some("error 2".to_string())).is_some());
        assert!(!ctx.can_retry());
        assert!(ctx.next_attempt(None).is_none());
    }

    #[test]
    fn test_per_server_rate_limits() {
        let limiter = RateLimiter::new();

        // Set different limits for different servers
        limiter.set_server_config("http://server1", RateLimitConfig::strict());
        limiter.set_server_config("http://server2", RateLimitConfig::permissive());

        // Both should work initially
        assert!(limiter.acquire("http://server1").is_ok());
        assert!(limiter.acquire("http://server2").is_ok());
    }
}
