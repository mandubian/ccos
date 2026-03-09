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

impl From<anyhow::Error> for ToolError {
    fn from(err: anyhow::Error) -> Self {
        let msg = err.to_string();

        // Classify the error based on its message/content
        if msg.contains("policy") || msg.contains("Permission Denied") || msg.contains("denied") {
            Self::permission(msg)
        } else if msg.contains("must not be empty")
            || msg.contains("Invalid")
            || msg.contains("must")
            || msg.contains("required")
            || msg.contains("denied by policy")
        {
            Self::validation(msg, Some("Check the tool schema and ensure all required fields are provided with valid values."))
        } else if msg.contains("not found")
            || msg.contains("File not found")
            || msg.contains("connection")
            || msg.contains("timeout")
        {
            Self::resource(msg, Some("Verify the resource exists or try again later."))
        } else if msg.contains("corrupted") || msg.contains("invariant") || msg.contains("unsafe") {
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
