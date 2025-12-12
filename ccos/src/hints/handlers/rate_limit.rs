//! Rate-limiting hint handler - limits execution frequency.

use crate::hints::types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
use crate::types::{Action, ActionType};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Handler for the `runtime.learning.rate-limit` hint.
///
/// Limits the frequency of capability execution using a token bucket algorithm.
///
/// # Hint Format
/// ```rtfs
/// ^{:runtime.learning.rate-limit {:requests-per-second 10 :burst 5}}
/// ```
///
/// # Fields
/// - `requests-per-second`: Maximum sustained request rate (default: 10)
/// - `burst`: Maximum burst capacity (default: 5)
pub struct RateLimitHintHandler {
    /// Token buckets per capability ID
    buckets: Mutex<HashMap<String, TokenBucket>>,
}

/// Simple token bucket for rate limiting
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    rate: f64,       // tokens per second
    max_tokens: f64, // burst capacity
}

impl TokenBucket {
    fn new(rate: f64, max_tokens: f64) -> Self {
        Self {
            tokens: max_tokens,
            last_refill: Instant::now(),
            rate,
            max_tokens,
        }
    }

    /// Attempts to consume one token. Returns true if successful.
    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Wait time until a token is available
    fn wait_time(&self) -> Duration {
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            let tokens_needed = 1.0 - self.tokens;
            let seconds = tokens_needed / self.rate;
            Duration::from_secs_f64(seconds)
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.max_tokens);
        self.last_refill = now;
    }
}

impl RateLimitHintHandler {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Extract f64 from a RTFS map value
    fn extract_f64_from_map(value: &Value, key: &str) -> Option<f64> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    };
                }
            }
        }
        None
    }
}

impl Default for RateLimitHintHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandler for RateLimitHintHandler {
    fn hint_key(&self) -> &str {
        "runtime.learning.rate-limit"
    }

    fn priority(&self) -> u32 {
        5 // Pre-execution layer - runs before retry (10)
    }

    fn validate_hint(&self, hint_value: &Value) -> RuntimeResult<()> {
        if let Value::Map(_) = hint_value {
            Ok(())
        } else {
            Err(RuntimeError::Generic(
                "rate-limit hint value must be a map".to_string(),
            ))
        }
    }

    fn apply<'a>(
        &'a self,
        host_call: &'a HostCall,
        hint_value: &'a Value,
        ctx: &'a ExecutionContext,
        next: NextExecutor<'a>,
    ) -> BoxFuture<'a, RuntimeResult<Value>> {
        Box::pin(async move {
            let rate =
                Self::extract_f64_from_map(hint_value, "requests-per-second").unwrap_or(10.0);
            let burst = Self::extract_f64_from_map(hint_value, "burst").unwrap_or(5.0);

            let capability_id = host_call.capability_id.clone();

            // Get or create token bucket for this capability
            let wait_time = {
                let mut buckets = self.buckets.lock().unwrap();
                let bucket = buckets
                    .entry(capability_id.clone())
                    .or_insert_with(|| TokenBucket::new(rate, burst));

                if bucket.try_consume() {
                    Duration::ZERO
                } else {
                    bucket.wait_time()
                }
            };

            // Wait if rate limited
            if wait_time > Duration::ZERO {
                // Log rate limiting
                if let Ok(mut chain) = ctx.causal_chain.lock() {
                    let _ = chain.append(
                        &Action::new(
                            ActionType::HintApplied,
                            format!("capability:{}", host_call.capability_id),
                            String::new(),
                        )
                        .with_metadata(
                            "hint",
                            &format!("rate-limit:waiting_{}ms", wait_time.as_millis()),
                        ),
                    );
                }

                tokio::time::sleep(wait_time).await;

                // Try to consume again after waiting
                let mut buckets = self.buckets.lock().unwrap();
                if let Some(bucket) = buckets.get_mut(&capability_id) {
                    if !bucket.try_consume() {
                        return Err(RuntimeError::Generic(format!(
                            "Rate limit exceeded for capability '{}'",
                            host_call.capability_id
                        )));
                    }
                }
            }

            // Log rate limit pass
            if let Ok(mut chain) = ctx.causal_chain.lock() {
                let _ = chain.append(
                    &Action::new(
                        ActionType::HintApplied,
                        format!("capability:{}", host_call.capability_id),
                        String::new(),
                    )
                    .with_metadata("hint", &format!("rate-limit:allowed (rate={}/s)", rate)),
                );
            }

            // Execute the next handler in the chain
            next().await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_handler_key_and_priority() {
        let handler = RateLimitHintHandler::new();
        assert_eq!(handler.hint_key(), "runtime.learning.rate-limit");
        assert_eq!(handler.priority(), 5); // Pre-execution, before retry
    }

    #[test]
    fn test_token_bucket_immediate() {
        let mut bucket = TokenBucket::new(10.0, 5.0);
        // Should allow burst of 5
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        // 6th should fail (no immediate tokens)
        assert!(!bucket.try_consume());
    }
}
