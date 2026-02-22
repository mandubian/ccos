//! Error refiner and classifier for code execution
//!
//! Parses execution output and classifies errors to provide better context
//! for the refinement loop.

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Classification of an execution error
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ErrorClass {
    /// No error detected
    None,
    /// Syntax error (e.g. invalid syntax, indentation)
    Syntax,
    /// Missing dependency (e.g. ModuleNotFoundError)
    MissingDependency(String),
    /// Network failure (e.g. DNS resolution error, connection refused, unreachable host)
    NetworkFailure(String),
    /// Runtime error (e.g. ValueError, TypeError, KeyError)
    Runtime(String),
    /// Timeout during execution
    Timeout,
    /// Resource limit exceeded (e.g. Memory)
    ResourceLimit(String),
    /// Security policy violation
    SecurityViolation(String),
    /// Unknown error
    Unknown,
}

/// A classified execution error with context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedError {
    /// The class of the error
    pub class: ErrorClass,
    /// The error message or trace
    pub message: String,
    /// Suggested fix if available
    pub suggestion: Option<String>,
}

pub struct ErrorRefiner {
    /// Regex for ModuleNotFoundError
    module_not_found_re: Regex,
    /// Regex for SyntaxError
    syntax_error_re: Regex,
    /// Regex for generic Python exceptions (NameError, TypeError, etc.)
    generic_exception_re: Regex,
}

impl ErrorRefiner {
    pub fn new() -> Self {
        Self {
            module_not_found_re: Regex::new(r"ModuleNotFoundError: No module named '([^']+)'")
                .unwrap(),
            syntax_error_re: Regex::new(r"SyntaxError: (.+)").unwrap(),
            generic_exception_re: Regex::new(r"^(\w+Error): (.+)").unwrap(),
        }
    }

    /// Detect network-related failures from common Python/system error messages
    fn is_network_error(&self, stderr: &str) -> Option<String> {
        // Patterns that reliably indicate network/DNS failure rather than logic errors
        const NETWORK_PATTERNS: &[&str] = &[
            "Temporary failure in name resolution",
            "Name or service not known",
            "socket.gaierror",
            "ConnectionRefusedError",
            "ConnectionError",
            "requests.exceptions.ConnectionError",
            "urllib.error.URLError",
            "httpx.ConnectError",
            "Network is unreachable",
            "No route to host",
            "Connection timed out",
        ];
        for pattern in NETWORK_PATTERNS {
            if stderr.contains(pattern) {
                return Some((*pattern).to_string());
            }
        }
        None
    }

    /// Classify a Python error from stderr
    pub fn classify_python_error(&self, stderr: &str) -> ClassifiedError {
        if stderr.is_empty() {
            return ClassifiedError {
                class: ErrorClass::None,
                message: String::new(),
                suggestion: None,
            };
        }

        // Check for network/DNS errors before generic exception matching
        if let Some(pattern) = self.is_network_error(stderr) {
            return ClassifiedError {
                class: ErrorClass::NetworkFailure(pattern.clone()),
                message: format!("Network failure detected: {}", pattern),
                suggestion: Some(
                    "Use a list of fallback URLs/mirrors. Loop over them with a short \
                     per-request timeout (5s). Catch each exception individually and \
                     only raise after all options are exhausted."
                    .to_string(),
                ),
            };
        }

        // Check for ModuleNotFoundError
        if let Some(caps) = self.module_not_found_re.captures(stderr) {
            let module = caps.get(1).map_or("", |m| m.as_str()).to_string();
            return ClassifiedError {
                class: ErrorClass::MissingDependency(module.clone()),
                message: format!("Module not found: {}", module),
                suggestion: Some(format!("Try adding '{}' to the dependencies list.", module)),
            };
        }

        // Check for SyntaxError
        if let Some(caps) = self.syntax_error_re.captures(stderr) {
            let details = caps.get(1).map_or("", |m| m.as_str()).to_string();
            return ClassifiedError {
                class: ErrorClass::Syntax,
                message: format!("Syntax error: {}", details),
                suggestion: Some("Check your code for typos or incorrect indentation.".to_string()),
            };
        }

        // Check for other common exceptions
        for line in stderr.lines().rev() {
            if let Some(caps) = self.generic_exception_re.captures(line) {
                let error_type = caps.get(1).map_or("", |m| m.as_str()).to_string();
                let message = caps.get(2).map_or("", |m| m.as_str()).to_string();
                return ClassifiedError {
                    class: ErrorClass::Runtime(error_type.clone()),
                    message: format!("{}: {}", error_type, message),
                    suggestion: None,
                };
            }
        }

        ClassifiedError {
            class: ErrorClass::Unknown,
            message: "An unknown error occurred during execution.".to_string(),
            suggestion: Some("Review the full logs for details.".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_module_not_found() {
        let refiner = ErrorRefiner::new();
        let stderr = "Traceback (most recent call last):\n  File \"script.py\", line 1, in <module>\n    import missing_lib\nModuleNotFoundError: No module named 'missing_lib'";
        let classified = refiner.classify_python_error(stderr);
        assert!(
            matches!(classified.class, ErrorClass::MissingDependency(ref m) if m == "missing_lib")
        );
    }

    #[test]
    fn test_classify_syntax_error() {
        let refiner = ErrorRefiner::new();
        let stderr = "  File \"script.py\", line 1\n    print(\"hello\"\n                ^\nSyntaxError: unexpected EOF while parsing";
        let classified = refiner.classify_python_error(stderr);
        assert_eq!(classified.class, ErrorClass::Syntax);
    }

    #[test]
    fn test_classify_runtime_error() {
        let refiner = ErrorRefiner::new();
        let stderr = "Traceback (most recent call last):\n  File \"script.py\", line 1, in <module>\n    1 / 0\nZeroDivisionError: division by zero";
        let classified = refiner.classify_python_error(stderr);
        assert!(matches!(classified.class, ErrorClass::Runtime(ref e) if e == "ZeroDivisionError"));
    }
}
