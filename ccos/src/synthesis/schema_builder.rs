//! Parameter Schema Builder
//!
//! Extracts parameter schemas from CausalChain by analyzing user.ask actions.
//! Implements the algorithm from spec section 23.1.

use super::InteractionTurn;
use std::collections::HashMap;

/// Complete parameter schema extracted from conversation.
#[derive(Debug, Clone)]
pub struct ParamSchema {
    pub params: HashMap<String, ParamMeta>,
}

/// Metadata for a single parameter.
#[derive(Debug, Clone)]
pub struct ParamMeta {
    /// Namespaced key (e.g., "trip/destination")
    pub key: String,
    /// Inferred type (string or enum)
    pub param_type: ParamTypeInfo,
    /// Whether this parameter is required
    pub required: bool,
    /// Turn number where first asked
    pub source_turn: usize,
    /// The prompt text shown to user
    pub prompt: String,
    /// The user's answer (if available)
    pub answer: Option<String>,
}

/// Type information for a parameter.
#[derive(Debug, Clone)]
pub enum ParamTypeInfo {
    String,
    Enum { values: Vec<String> },
}

/// Extract parameter schema from conversation history.
///
/// # Algorithm (from spec 23.1)
/// 1. Query CausalChain for user.ask actions
/// 2. For each (turn, prompt, answer):
///    - Infer key via heuristics (e.g., "destination" → "trip/destination")
///    - Detect enum via "(low / medium / high)" pattern
///    - Mark required based on usage
/// 3. Return ParamSchema with provenance
pub fn extract_param_schema(conversation: &[InteractionTurn]) -> ParamSchema {
    // MVP implementation for Phase 5: derive parameter schema heuristically
    // from the supplied conversation turns. This avoids heavy CCOS setup
    // in unit tests while still following spec heuristics.
    let mut params: HashMap<String, ParamMeta> = HashMap::new();

    for turn in conversation.iter() {
        let prompt = turn.prompt.trim();
        if prompt.is_empty() {
            continue;
        }

        // Infer a namespaced key for this prompt
        let key = namespace_infer(prompt);

        // Skip if we've already recorded this parameter (but update answered value if present)
        if let Some(existing) = params.get_mut(&key) {
            // update answer if newly available
            if existing.answer.is_none() {
                if let Some(ans) = &turn.answer {
                    existing.answer = Some(ans.clone());
                }
            }
            existing.source_turn = existing.source_turn.min(turn.turn_index);
            existing.required = existing.required || turn.answer.is_none();
            continue;
        }

        // Try to detect enumerations in the prompt
        let enum_vals = extract_enumeration(prompt);
        let param_type = match enum_vals.as_ref() {
            Some(vs) if !vs.is_empty() => ParamTypeInfo::Enum { values: vs.clone() },
            _ => ParamTypeInfo::String,
        };

        let meta = ParamMeta {
            key: key.clone(),
            param_type,
            required: true, // asked questions are considered required by default
            source_turn: turn.turn_index,
            prompt: prompt.to_string(),
            answer: turn.answer.clone(),
        };

        params.insert(key, meta);
    }

    ParamSchema { params }
}

/// Infer namespaced parameter key from prompt text.
///
/// Heuristics:
/// - "destination" / "where" → "trip/destination"
/// - "dates" / "when" → "trip/dates"
/// - "budget" / "how much" → "trip/budget"
/// - "risk tolerance" → "investment/risk_tolerance"
fn namespace_infer(prompt: &str) -> String {
    let lower = prompt.to_lowercase();

    if lower.contains("destination")
        || lower.contains("where do you")
        || lower.contains("where would")
        || lower.contains("which city")
    {
        "trip/destination".to_string()
    } else if lower.contains("date")
        || lower.contains("when")
        || lower.contains("which day")
        || lower.contains("what dates")
    {
        "trip/dates".to_string()
    } else if lower.contains("budget")
        || lower.contains("how much")
        || lower.contains("cost")
        || lower.contains("price")
    {
        "trip/budget".to_string()
    } else if lower.contains("risk tolerance") || lower.contains("risk") {
        "investment/risk_tolerance".to_string()
    } else if lower.contains("name") || lower.contains("what is your name") {
        "person/name".to_string()
    } else if lower.contains("interest")
        || lower.contains("interests")
        || lower.contains("what are your interests")
    {
        "trip/interests".to_string()
    } else if lower.contains("duration") || lower.contains("how long") || lower.contains("length") {
        "trip/duration".to_string()
    } else if lower.contains("email") {
        "contact/email".to_string()
    } else if lower.contains("phone") || lower.contains("phone number") {
        "contact/phone".to_string()
    } else {
        // Fallback: create an 'unknown' namespace using the first alphanumeric token
        let token = lower
            .split(|c: char| !c.is_alphanumeric())
            .find(|s| !s.is_empty())
            .unwrap_or("param");
        format!("unknown/{}", token)
    }
}

/// Extract enumeration values from prompt text.
///
/// Detects patterns like:
/// - "(low / medium / high)"
/// - "low, medium, or high"
/// - "art / food / history"
fn extract_enumeration(prompt: &str) -> Option<Vec<String>> {
    let p = prompt.trim();
    // Pattern a / b / c
    if p.contains("/") {
        let parts: Vec<String> = p
            .split('/')
            .map(|s| {
                s.trim().trim_matches(|c: char| {
                    c == '(' || c == ')' || c == '?' || c == '"' || c == '\''
                })
            })
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.trim()
                    .trim_end_matches(|c: char| c == '.' || c == ',')
                    .to_string()
            })
            .collect();
        // Heuristic: only treat as enum if at least 2 items and short tokens
        if parts.len() >= 2 && parts.iter().all(|s| s.len() <= 30) {
            return Some(parts);
        }
    }

    // Pattern: a, b, or c  OR a, b and c
    if p.contains(',') && (p.contains(" or ") || p.contains(" and ")) {
        // Split on commas first
        let mut parts: Vec<String> = p
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // If the last segment contains ' or ' or ' and ', split it further
        if let Some(last) = parts.last().cloned() {
            if last.contains(" or ") {
                let mut tail: Vec<String> =
                    last.split(" or ").map(|s| s.trim().to_string()).collect();
                parts.pop();
                parts.append(&mut tail);
            } else if last.contains(" and ") {
                let mut tail: Vec<String> =
                    last.split(" and ").map(|s| s.trim().to_string()).collect();
                parts.pop();
                parts.append(&mut tail);
            }
        }

        let parts_clean: Vec<String> = parts
            .into_iter()
            .map(|s| {
                s.trim_matches(|c: char| c == '.' || c == '?' || c == '"')
                    .to_string()
            })
            .collect();
        if parts_clean.len() >= 2 {
            return Some(parts_clean);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthesis::InteractionTurn;

    fn make_turn(i: usize, prompt: &str, ans: Option<&str>) -> InteractionTurn {
        InteractionTurn {
            turn_index: i,
            prompt: prompt.to_string(),
            answer: ans.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn test_extract_basic_params() {
        let convo = vec![
            make_turn(0, "Where would you like to go?", Some("Paris")),
            make_turn(1, "What dates will you travel?", None),
            make_turn(
                2,
                "What are your interests (art / food / history)?",
                Some("art"),
            ),
            make_turn(
                3,
                "What's your budget? (low, medium, or high)",
                Some("medium"),
            ),
            make_turn(4, "Please provide your name.", Some("Jane")),
        ];

        let schema = extract_param_schema(&convo);

        // destination
        let dest = schema
            .params
            .get("trip/destination")
            .expect("destination present");
        assert!(matches!(dest.param_type, ParamTypeInfo::String));
        assert_eq!(dest.answer.as_deref(), Some("Paris"));

        // dates
        let dates = schema.params.get("trip/dates").expect("dates present");
        assert!(matches!(dates.param_type, ParamTypeInfo::String));
        assert!(dates.answer.is_none());

        // interests (enum)
        let inter = schema
            .params
            .get("trip/interests")
            .expect("interests present");
        match &inter.param_type {
            ParamTypeInfo::Enum { values } => {
                assert!(values.iter().any(|v| v.to_lowercase().contains("art")));
            }
            _ => panic!("expected enum for interests"),
        }
        assert_eq!(inter.answer.as_deref(), Some("art"));

        // budget (enum)
        let budget = schema.params.get("trip/budget").expect("budget present");
        match &budget.param_type {
            ParamTypeInfo::Enum { values } => {
                assert!(values.iter().any(|v| v.to_lowercase().contains("low")));
            }
            _ => panic!("expected enum for budget"),
        }

        // name
        let name = schema.params.get("person/name").expect("name present");
        assert!(matches!(name.param_type, ParamTypeInfo::String));
        assert_eq!(name.answer.as_deref(), Some("Jane"));
    }
}
