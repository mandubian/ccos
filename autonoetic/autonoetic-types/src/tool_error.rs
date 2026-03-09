//! Tool error types for structured failure feedback.

use serde::{Deserialize, Serialize};

/// Type of tool error, indicating whether it's recoverable or fatal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorType {
    /// Validation error: malformed input, missing required field, policy denial.
    /// The agent can repair and retry.
    Validation,
    /// Permission error: agent lacks required capability or scope.
    /// The agent can request additional authorization or adjust scope.
    Permission,
    /// Resource error: missing file, unavailable service, rate limit.
    /// The agent can retry with backoff or use alternative.
    Resource,
    /// Execution error: tool ran but produced an unexpected result.
    /// The agent can inspect and adjust.
    Execution,
    /// Fatal error: corrupted state, invariant violation, unsafe condition.
    /// The agent session should abort; this is not recoverable.
    Fatal,
}

impl std::fmt::Display for ToolErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolErrorType::Validation => write!(f, "validation"),
            ToolErrorType::Permission => write!(f, "permission"),
            ToolErrorType::Resource => write!(f, "resource"),
            ToolErrorType::Execution => write!(f, "execution"),
            ToolErrorType::Fatal => write!(f, "fatal"),
        }
    }
}

/// Structured tool error response for agent feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    /// Always false for errors.
    #[serde(rename = "ok")]
    pub success: bool,
    /// Type of error indicating recoverability.
    pub error_type: ToolErrorType,
    /// Human-readable error message.
    pub message: String,
    /// Optional hint for repairing the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_hint: Option<String>,
    /// Optional original error details (for logging, not always exposed to agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ToolError {
    /// Creates a new validation error.
    pub fn validation(message: impl Into<String>, repair_hint: Option<impl Into<String>>) -> Self {
        Self {
            success: false,
            error_type: ToolErrorType::Validation,
            message: message.into(),
            repair_hint: repair_hint.map(|h| h.into()),
            details: None,
        }
    }

    /// Creates a new permission error.
    pub fn permission(message: impl Into<String>) -> Self {
        Self {
            success: false,
            error_type: ToolErrorType::Permission,
            message: message.into(),
            repair_hint: Some(
                "Request additional authorization or adjust the scope of your request.".to_string(),
            ),
            details: None,
        }
    }

    /// Creates a new resource error.
    pub fn resource(message: impl Into<String>, repair_hint: Option<impl Into<String>>) -> Self {
        Self {
            success: false,
            error_type: ToolErrorType::Resource,
            message: message.into(),
            repair_hint: repair_hint.map(|h| h.into()),
            details: None,
        }
    }

    /// Creates a new execution error.
    pub fn execution(message: impl Into<String>, repair_hint: Option<impl Into<String>>) -> Self {
        Self {
            success: false,
            error_type: ToolErrorType::Execution,
            message: message.into(),
            repair_hint: repair_hint.map(|h| h.into()),
            details: None,
        }
    }

    /// Creates a new fatal error.
    pub fn fatal(message: impl Into<String>, details: Option<impl Into<String>>) -> Self {
        Self {
            success: false,
            error_type: ToolErrorType::Fatal,
            message: message.into(),
            repair_hint: None,
            details: details.map(|d| d.into()),
        }
    }

    /// Returns true if this error is recoverable (agent can retry).
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self.error_type,
            ToolErrorType::Validation
                | ToolErrorType::Permission
                | ToolErrorType::Resource
                | ToolErrorType::Execution
        )
    }

    /// Converts the error to a JSON string for tool_result.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| {
            format!(
                r#"{{"ok":false,"error_type":"fatal","message":"Failed to serialize error: {}"}}"#,
                e
            )
        })
    }
}

/// Helper to create tagged errors with explicit error type classification.
/// Use these functions instead of anyhow::anyhow! for tool errors to ensure
/// proper classification without relying on string heuristics.
pub mod tagged {
    use super::*;
    use std::error::Error;

    /// A wrapper that attaches error type metadata to an anyhow::Error.
    #[derive(Debug)]
    pub struct Tagged {
        error_type: ToolErrorType,
        source: anyhow::Error,
    }

    // SAFETY: Tagged is safe to send across thread boundaries because:
    // - The inner anyhow::Error is wrapped in a concrete owned type with no interior mutability
    // - The error_type field is Clone + Send + Sync (ToolErrorType derives both)
    // - No references are held that could become invalid across threads
    unsafe impl Send for Tagged {}
    unsafe impl Sync for Tagged {}

    impl Tagged {
        pub fn validation(err: impl Into<anyhow::Error>) -> Self {
            Self {
                error_type: ToolErrorType::Validation,
                source: err.into(),
            }
        }

        pub fn permission(err: impl Into<anyhow::Error>) -> Self {
            Self {
                error_type: ToolErrorType::Permission,
                source: err.into(),
            }
        }

        pub fn resource(err: impl Into<anyhow::Error>) -> Self {
            Self {
                error_type: ToolErrorType::Resource,
                source: err.into(),
            }
        }

        pub fn execution(err: impl Into<anyhow::Error>) -> Self {
            Self {
                error_type: ToolErrorType::Execution,
                source: err.into(),
            }
        }

        pub fn fatal(err: impl Into<anyhow::Error>) -> Self {
            Self {
                error_type: ToolErrorType::Fatal,
                source: err.into(),
            }
        }
    }

    impl std::fmt::Display for Tagged {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}: {}", self.error_type, self.source)
        }
    }

    impl Error for Tagged {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(self.source.as_ref())
        }
    }

    impl Tagged {
        /// Extracts the error type and message from this tagged error.
        pub fn into_parts(self) -> (ToolErrorType, String) {
            (self.error_type.clone(), self.source.to_string())
        }
    }
}

impl From<tagged::Tagged> for ToolError {
    fn from(tagged: tagged::Tagged) -> Self {
        // Extract the error type and message from the tagged error
        let (error_type, message) = tagged.into_parts();
        match error_type {
            ToolErrorType::Validation => Self::validation(message, None::<String>),
            ToolErrorType::Permission => Self::permission(message),
            ToolErrorType::Resource => Self::resource(message, None::<String>),
            ToolErrorType::Execution => Self::execution(message, None::<String>),
            ToolErrorType::Fatal => {
                let msg2 = message.clone();
                Self::fatal(message, Some(msg2))
            }
        }
    }
}

impl From<anyhow::Error> for ToolError {
    fn from(err: anyhow::Error) -> Self {
        // Check if this is a tagged error by looking at the error chain
        for cause in err.chain() {
            let msg = cause.to_string();
            let msg_trimmed = msg.trim();
            if msg.starts_with("validation:") {
                let inner = msg.strip_prefix("validation:").unwrap_or(&msg);
                // Add repair hint for common validation patterns
                let repair_hint = if msg_trimmed.contains("must not be empty") {
                    Some("Ensure all required fields are provided and not empty.".to_string())
                } else if msg_trimmed.contains("Invalid JSON") {
                    Some("Check the tool schema and ensure JSON is valid.".to_string())
                } else {
                    None
                };
                return Self::validation(inner.to_string(), repair_hint);
            } else if msg.starts_with("permission:") {
                let inner = msg.strip_prefix("permission:").unwrap_or(&msg);
                return Self::permission(inner.to_string());
            } else if msg.starts_with("resource:") {
                let inner = msg.strip_prefix("resource:").unwrap_or(&msg);
                return Self::resource(inner.to_string(), None::<String>);
            } else if msg.starts_with("execution:") {
                let inner = msg.strip_prefix("execution:").unwrap_or(&msg);
                return Self::execution(inner.to_string(), None::<String>);
            } else if msg.starts_with("fatal:") {
                let inner = msg.strip_prefix("fatal:").unwrap_or(&msg);
                return Self::fatal(inner.to_string(), Some(err.to_string()));
            }
        }

        // Fall back to string-based classification for untagged errors
        let msg = err.to_string();
        let msg_trimmed = msg.trim();
        if msg.contains("policy") || msg.contains("Permission Denied") || msg.contains("denied") {
            Self::permission(msg)
        } else if msg_trimmed.contains("must not be empty")
            || msg_trimmed.contains("Invalid")
            || msg_trimmed.contains("must")
            || msg_trimmed.contains("required")
            || msg_trimmed.contains("denied by policy")
        {
            Self::validation(msg, Some("Check the tool schema and ensure all required fields are provided with valid values."))
        } else if msg_trimmed.contains("not found")
            || msg_trimmed.contains("File not found")
            || msg_trimmed.contains("connection")
            || msg_trimmed.contains("timeout")
        {
            Self::resource(msg, Some("Verify the resource exists or try again later."))
        } else if msg_trimmed.contains("corrupted")
            || msg_trimmed.contains("invariant")
            || msg_trimmed.contains("unsafe")
            || msg_trimmed.contains("Unknown tool")
        {
            Self::fatal(msg, Some(err.to_string()))
        } else {
            // Default to execution error for unknown types
            Self::execution(msg, None::<String>)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error() {
        let err = ToolError::validation(
            "missing field 'id'",
            Some("Include an 'id' field in your request"),
        );
        assert!(!err.success);
        assert_eq!(err.error_type, ToolErrorType::Validation);
        assert!(err.is_recoverable());
        assert!(err.repair_hint.is_some());
    }

    #[test]
    fn test_fatal_error() {
        let err = ToolError::fatal("corrupted state", Some("state hash mismatch"));
        assert!(!err.success);
        assert_eq!(err.error_type, ToolErrorType::Fatal);
        assert!(!err.is_recoverable());
        assert!(err.repair_hint.is_none());
    }

    #[test]
    fn test_error_to_json() {
        let err = ToolError::validation("bad input", Some("fix it"));
        let json = err.to_json_string();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get("ok").unwrap(), false);
        assert_eq!(parsed.get("error_type").unwrap(), "validation");
    }

    #[test]
    fn test_anyhow_conversion() {
        let anyhow_err = anyhow::anyhow!("memory read denied by policy");
        let err: ToolError = anyhow_err.into();
        assert_eq!(err.error_type, ToolErrorType::Permission);
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_validation_conversion() {
        let anyhow_err = anyhow::anyhow!("id must not be empty");
        let err: ToolError = anyhow_err.into();
        assert_eq!(err.error_type, ToolErrorType::Validation);
        assert!(err.is_recoverable());
    }
}
