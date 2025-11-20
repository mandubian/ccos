use regex::Regex;
use rtfs::runtime::error::RuntimeError;

/// Structured diagnostics for RTFS errors that can be surfaced to an LLM.
#[derive(Debug, Clone)]
pub struct RtfsErrorDiagnostics {
    pub summary: String,
    pub snippet: Option<String>,
    pub hints: Vec<String>,
}

/// Utilities for turning RTFS compiler/runtime errors into LLM-friendly guidance.
pub struct RtfsErrorExplainer;

impl RtfsErrorExplainer {
    /// Attempt to explain the provided runtime error. Returns `None` if no targeted
    /// diagnostics are available for this error category.
    pub fn explain(error: &RuntimeError) -> Option<RtfsErrorDiagnostics> {
        match error {
            RuntimeError::Generic(message)
                if message.contains("Failed to parse")
                    || message.contains("ParsingError")
                    || message.contains("PestError") =>
            {
                Some(Self::explain_parse_failure(message))
            }
            RuntimeError::InvalidProgram(message)
                if message.to_ascii_lowercase().contains("syntax")
                    || message.to_ascii_lowercase().contains("parse") =>
            {
                Some(Self::explain_parse_failure(message))
            }
            RuntimeError::TypeValidationError(message) => {
                Some(Self::basic_type_validation_guidance(message))
            }
            RuntimeError::Generic(message) if message.contains("Input validation failed")
                || message.to_ascii_lowercase().contains("type mismatch") =>
            {
                // Build base guidance from existing type validation helper and append
                // the JSON parsing hint to describe how to convert string outputs into vectors.
                let mut base = Self::basic_type_validation_guidance(message).hints;
                base.push(Self::basic_collection_mismatch_hint());
                Some(RtfsErrorDiagnostics {
                    summary: "Input validation failed when invoking a capability; possible type mismatch.".to_string(),
                    snippet: None,
                    hints: base,
                })
            }
            RuntimeError::UndefinedSymbol(symbol) => Some(RtfsErrorDiagnostics {
                summary: format!("Undefined symbol `{}` encountered during execution.", symbol.0),
                snippet: None,
                hints: vec![
                    "Ensure every symbol is defined before use (e.g. via `let` or as a capability output).".to_string(),
                    "Verify that previous steps expose the expected output keys.".to_string(),
                ],
            }),
            RuntimeError::SymbolNotFound(symbol) => Some(RtfsErrorDiagnostics {
                summary: format!("Symbol `{}` was not found in the current scope.", symbol),
                snippet: None,
                hints: vec![
                    "Use fully-qualified capability IDs (e.g. `:github.issues.list`) in `call` forms.".to_string(),
                    "Ensure the plan bindings match the capability's declared output keys.".to_string(),
                ],
            }),
            _ => None,
        }
    }

    fn explain_parse_failure(message: &str) -> RtfsErrorDiagnostics {
        let snippet = Self::extract_problematic_line(message);
        let mut hints = Vec::new();

        hints.push("RTFS maps use `{:keyword value}` pairs (no `=`). Example: `(call :service.id {:param \"value\" :count 3})`.".to_string());
        hints.push("Strings must be quoted with double quotes (e.g. `\"mandubian\"`).".to_string());
        hints.push("Step skeleton: `(step \"Name\" (call :provider.capability {:arg1 v1 :arg2 \"text\"}))`.".to_string());

        if snippet
            .as_deref()
            .map(|line| line.contains(" = ") || line.contains("= "))
            .unwrap_or(false)
        {
            hints.push("Remove `=` inside maps. Replace `:username = mandubian` with `:username \"mandubian\"`.".to_string());
        }

        if snippet
            .as_deref()
            .map(|line| line.contains(":") && !line.contains(" :"))
            .unwrap_or(false)
        {
            hints.push(
                "Use spaces before keywords: `{ :key value }` rather than `{key value}`."
                    .to_string(),
            );
        }

        RtfsErrorDiagnostics {
            summary: "RTFS parser rejected the plan; fix syntax issues before retrying."
                .to_string(),
            snippet,
            hints,
        }
    }

    fn basic_type_validation_guidance(_message: &str) -> RtfsErrorDiagnostics {
        let mut hints = Vec::new();
        hints.push(
            "Ensure capability inputs refer to the correct fields (e.g. `:issues` instead of `:github_issues`)."
                .to_string(),
        );
        hints.push(
            "When reading capability outputs, wrap access in `(let [res step_0] (get res :outputs))` if needed."
                .to_string(),
        );

        RtfsErrorDiagnostics {
            summary: "Type validation failed when compiling the RTFS plan.".to_string(),
            snippet: None,
            hints,
        }
    }

    fn basic_collection_mismatch_hint() -> String {
        "If a capability returns a JSON string inside a field like `:content`, parse it before passing it to a step expecting a vector. Example: `(parse-json (get step_0 :content))` and then pass the parsed vector to `:collection` for `mcp.core.filter`. Use the stdlib `parse-json` function directly, not a capability call.".to_string()
    }

    fn extract_problematic_line(message: &str) -> Option<String> {
        let regex = Regex::new(r#"line:\s*"([^"]+)""#).ok()?;
        regex
            .captures(message)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()))
            .filter(|line| !line.is_empty())
    }

    /// Render diagnostics into a single human/LLM-readable string.
    pub fn format_for_llm(diag: &RtfsErrorDiagnostics) -> String {
        let mut output = String::new();
        output.push_str(&diag.summary);
        output.push('\n');
        if let Some(snippet) = &diag.snippet {
            output.push_str("Problematic line:\n");
            output.push_str(snippet);
            output.push('\n');
        }
        if !diag.hints.is_empty() {
            output.push_str("Hints:\n");
            for hint in &diag.hints {
                output.push_str("• ");
                output.push_str(hint);
                output.push('\n');
            }
        }
        output
    }

    // tests for the explainer are maintained in separate integration tests; we
    // prefer exercising the LLM repair behavior end-to-end rather than unit tests
    // for hints that bridge to generated capability behavior.
}

// no explicit tests for collection hint; prefer LLM-driven repairs in practice
