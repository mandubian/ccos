//! Retry hint handler - executes with exponential backoff on failure.

use crate::hints::types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
use crate::types::{Action, ActionType};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::{RuntimeError, RuntimeResult};

/// Handler for the `runtime.learning.retry` hint.
///
/// Retries failed capability executions with exponential backoff.
///
/// # Hint Format
/// ```rtfs
/// ^{:runtime.learning.retry {:max-retries 3 :backoff-ms 100}}
/// ```
///
/// # Fields
/// - `max-retries`: Maximum number of retry attempts (default: 3)
/// - `backoff-ms`: Base backoff time in milliseconds (default: 100)
pub struct RetryHintHandler;

impl RetryHintHandler {
    pub fn new() -> Self {
        Self
    }

    /// Extract u64 from a RTFS map value
    fn extract_u64_from_map(value: &Value, key: &str) -> Option<u64> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Integer(i) => Some(*i as u64),
                        Value::Float(f) => Some(*f as u64),
                        _ => None,
                    };
                }
            }
        }
        None
    }
}

impl Default for RetryHintHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandler for RetryHintHandler {
    fn hint_key(&self) -> &str {
        "runtime.learning.retry"
    }

    fn priority(&self) -> u32 {
        10 // Resilience layer - runs first (outermost wrapper)
    }

    fn validate_hint(&self, hint_value: &Value) -> RuntimeResult<()> {
        if let Value::Map(_) = hint_value {
            Ok(())
        } else {
            Err(RuntimeError::Generic(
                "retry hint value must be a map".to_string(),
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
            let max_retries = Self::extract_u64_from_map(hint_value, "max-retries").unwrap_or(3);
            let backoff_ms = Self::extract_u64_from_map(hint_value, "backoff-ms").unwrap_or(100);

            // First attempt - consume the original next
            let first_result = next().await;

            match first_result {
                Ok(value) => return Ok(value),
                Err(e) => {
                    let mut last_error = e;

                    // Retry attempts
                    for attempt in 1..=max_retries {
                        // Log retry attempt
                        if let Ok(mut chain) = ctx.causal_chain.lock() {
                            let _ = chain.append(
                                &Action::new(
                                    ActionType::HintApplied,
                                    format!("capability:{}", host_call.capability_id),
                                    String::new(),
                                )
                                .with_metadata(
                                    "hint",
                                    &format!("retry:attempt_{}_of_{}", attempt, max_retries),
                                ),
                            );
                        }

                        // Exponential backoff
                        let delay = backoff_ms * (1 << (attempt - 1));
                        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;

                        // Re-execute via marketplace (we can't reuse `next` - it's consumed)
                        let retry_result = ctx
                            .capability_marketplace
                            .execute_capability_enhanced(
                                &host_call.capability_id,
                                &Value::List(host_call.args.clone()),
                                host_call.metadata.as_ref(),
                            )
                            .await;

                        match retry_result {
                            Ok(value) => return Ok(value),
                            Err(e) => last_error = e,
                        }
                    }

                    Err(RuntimeError::Generic(format!(
                        "Capability '{}' failed after {} retries: {}",
                        host_call.capability_id, max_retries, last_error
                    )))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_handler_key_and_priority() {
        let handler = RetryHintHandler::new();
        assert_eq!(handler.hint_key(), "runtime.learning.retry");
        assert_eq!(handler.priority(), 10);
    }
}
