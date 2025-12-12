//! Fallback hint handler - tries alternative capability on failure.

use crate::hints::types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
use crate::types::{Action, ActionType};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::{RuntimeError, RuntimeResult};

/// Handler for the `runtime.learning.fallback` hint.
///
/// Executes an alternative capability if the primary one fails.
///
/// # Hint Format
/// ```rtfs
/// ^{:runtime.learning.fallback {:capability "backup.service" :reason "primary unreliable"}}
/// ```
///
/// # Fields
/// - `capability`: The fallback capability to use (required)
/// - `reason`: Description of why fallback might be needed (optional)
pub struct FallbackHintHandler;

impl FallbackHintHandler {
    pub fn new() -> Self {
        Self
    }

    /// Extract string from a RTFS map value
    fn extract_string_from_map(value: &Value, key: &str) -> Option<String> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    };
                }
            }
        }
        None
    }
}

impl Default for FallbackHintHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandler for FallbackHintHandler {
    fn hint_key(&self) -> &str {
        "runtime.learning.fallback"
    }

    fn priority(&self) -> u32 {
        30 // Error handling layer - runs after timeout
    }

    fn validate_hint(&self, hint_value: &Value) -> RuntimeResult<()> {
        if let Value::Map(_) = hint_value {
            // Check for required 'capability' field
            if Self::extract_string_from_map(hint_value, "capability").is_none() {
                return Err(RuntimeError::Generic(
                    "fallback hint requires 'capability' field".to_string(),
                ));
            }
            Ok(())
        } else {
            Err(RuntimeError::Generic(
                "fallback hint value must be a map".to_string(),
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
            // Try the primary execution first
            let result = next().await;

            // If successful, return immediately
            if result.is_ok() {
                return result;
            }

            // Primary failed - try fallback
            let fallback_capability = match Self::extract_string_from_map(hint_value, "capability")
            {
                Some(cap) => cap,
                None => {
                    return Err(RuntimeError::Generic(
                        "Fallback hint missing 'capability' field".to_string(),
                    ));
                }
            };

            let reason = Self::extract_string_from_map(hint_value, "reason")
                .unwrap_or_else(|| "primary failed".to_string());

            // Log fallback application
            if let Ok(mut chain) = ctx.causal_chain.lock() {
                let _ = chain.append(
                    &Action::new(
                        ActionType::HintApplied,
                        format!("capability:{}", host_call.capability_id),
                        String::new(),
                    )
                    .with_metadata(
                        "hint",
                        &format!("fallback:{} ({})", fallback_capability, reason),
                    ),
                );
            }

            // Execute fallback capability with same args
            ctx.capability_marketplace
                .execute_capability_enhanced(
                    &fallback_capability,
                    &Value::List(host_call.args.clone()),
                    host_call.metadata.as_ref(),
                )
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_handler_key_and_priority() {
        let handler = FallbackHintHandler::new();
        assert_eq!(handler.hint_key(), "runtime.learning.fallback");
        assert_eq!(handler.priority(), 30);
    }
}
