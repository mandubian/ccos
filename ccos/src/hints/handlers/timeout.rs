//! Timeout hint handler - applies time limits to capability execution.

use crate::hints::types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
use crate::types::{Action, ActionType};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::{RuntimeError, RuntimeResult};

/// Handler for the `runtime.learning.timeout` hint.
///
/// Applies a time limit to capability execution.
///
/// # Hint Format
/// ```rtfs
/// ^{:runtime.learning.timeout {:absolute-ms 5000}}
/// ```
///
/// # Fields
/// - `absolute-ms`: Maximum execution time in milliseconds (default: 30000)
pub struct TimeoutHintHandler;

impl TimeoutHintHandler {
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

impl Default for TimeoutHintHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandler for TimeoutHintHandler {
    fn hint_key(&self) -> &str {
        "runtime.learning.timeout"
    }

    fn priority(&self) -> u32 {
        20 // Resource limits layer - runs after retry
    }

    fn validate_hint(&self, hint_value: &Value) -> RuntimeResult<()> {
        if let Value::Map(_) = hint_value {
            Ok(())
        } else {
            Err(RuntimeError::Generic(
                "timeout hint value must be a map".to_string(),
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
            let timeout_ms = Self::extract_u64_from_map(hint_value, "absolute-ms").unwrap_or(30000);

            // Log timeout application
            if let Ok(mut chain) = ctx.causal_chain.lock() {
                let _ = chain.append(
                    &Action::new(
                        ActionType::HintApplied,
                        format!("capability:{}", host_call.capability_id),
                        String::new(),
                    )
                    .with_metadata("hint", &format!("timeout:{}ms", timeout_ms)),
                );
            }

            // Apply timeout to the next executor
            match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), next()).await {
                Ok(result) => result,
                Err(_) => Err(RuntimeError::Generic(format!(
                    "Capability '{}' timed out after {}ms",
                    host_call.capability_id, timeout_ms
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_handler_key_and_priority() {
        let handler = TimeoutHintHandler::new();
        assert_eq!(handler.hint_key(), "runtime.learning.timeout");
        assert_eq!(handler.priority(), 20);
    }
}
