use crate::types::Intent;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Extract the first top-level `(intent …)` s-expression from the given text.
/// Returns `None` if no well-formed intent block is found.
pub fn extract_intent(text: &str) -> Option<String> {
    // Locate the starting position of the "(intent" keyword
    let start = text.find("(intent")?;

    // Scan forward and track parenthesis depth to find the matching ')'
    let mut depth = 0usize;
    for (idx, ch) in text[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                // When we return to depth 0 we've closed the original "(intent"
                if depth == 0 {
                    let end = start + idx + 1; // inclusive of current ')'
                    return Some(text[start..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Replace #rx"pattern" literals with plain "pattern" string literals so the current
/// grammar (which lacks regex literals) can parse the intent.
pub fn sanitize_regex_literals(text: &str) -> String {
    // Matches #rx"..." with minimal escaping (no nested quotes inside pattern)
    let re = regex::Regex::new(r#"#rx\"([^\"]*)\""#).unwrap();
    re.replace_all(text, |caps: &regex::Captures| format!("\"{}\"", &caps[1]))
        .into_owned()
}

/// Convert parser Literal to runtime Value (basic subset)
pub fn lit_to_val(lit: &rtfs::ast::Literal) -> Value {
    use rtfs::ast::Literal as Lit;
    match lit {
        Lit::String(s) => Value::String(s.clone()),
        Lit::Integer(i) => Value::Integer(*i),
        Lit::Float(f) => Value::Float(*f),
        Lit::Boolean(b) => Value::Boolean(*b),
        _ => Value::Nil,
    }
}

pub fn expr_to_value(expr: &rtfs::ast::Expression) -> Value {
    use rtfs::ast::Expression as E;
    match expr {
        E::Literal(lit) => lit_to_val(lit),
        E::Map(m) => {
            let mut map = std::collections::HashMap::new();
            for (k, v) in m {
                map.insert(k.clone(), expr_to_value(v));
            }
            Value::Map(map)
        }
        E::Vector(vec) | E::List(vec) => {
            let vals = vec.iter().map(expr_to_value).collect();
            if matches!(expr, E::Vector(_)) {
                Value::Vector(vals)
            } else {
                Value::List(vals)
            }
        }
        E::Symbol(s) => Value::Symbol(rtfs::ast::Symbol(s.0.clone())),
        E::FunctionCall { callee, arguments } => {
            // Convert function calls to a list representation for storage
            let mut func_list = vec![expr_to_value(callee)];
            func_list.extend(arguments.iter().map(expr_to_value));
            Value::List(func_list)
        }
        E::Fn(fn_expr) => {
            // Convert fn expressions to a list representation: (fn params body...)
            let mut fn_list = vec![Value::Symbol(rtfs::ast::Symbol("fn".to_string()))];

            // Add parameters as a vector
            let mut params = Vec::new();
            for param in &fn_expr.params {
                params.push(Value::Symbol(rtfs::ast::Symbol(format!(
                    "{:?}",
                    param.pattern
                ))));
            }
            fn_list.push(Value::Vector(params));

            // Add body expressions
            for body_expr in &fn_expr.body {
                fn_list.push(expr_to_value(body_expr));
            }

            Value::List(fn_list)
        }
        _ => Value::Nil,
    }
}

pub fn map_expr_to_string_value(
    expr: &rtfs::ast::Expression,
) -> Option<std::collections::HashMap<String, Value>> {
    use rtfs::ast::{Expression as E, MapKey};
    if let E::Map(m) = expr {
        let mut out = std::collections::HashMap::new();
        for (k, v) in m {
            let key_str = match k {
                MapKey::Keyword(k) => k.0.clone(),
                MapKey::String(s) => s.clone(),
                MapKey::Integer(i) => i.to_string(),
            };
            out.insert(key_str, expr_to_value(v));
        }
        Some(out)
    } else {
        None
    }
}

pub fn intent_from_function_call(expr: &rtfs::ast::Expression) -> Option<Intent> {
    use rtfs::ast::{Expression as E, Literal, Symbol};

    let E::FunctionCall { callee, arguments } = expr else {
        return None;
    };
    let E::Symbol(Symbol(sym)) = &**callee else {
        return None;
    };
    if sym != "intent" {
        return None;
    }
    if arguments.is_empty() {
        return None;
    }

    // The first argument is the intent name/type, can be either a symbol or string literal
    let name = if let E::Symbol(Symbol(name_sym)) = &arguments[0] {
        name_sym.clone()
    } else if let E::Literal(Literal::String(name_str)) = &arguments[0] {
        name_str.clone()
    } else {
        return None; // First argument must be a symbol or string
    };

    let mut properties = HashMap::new();
    let mut args_iter = arguments[1..].chunks_exact(2);
    while let Some([key_expr, val_expr]) = args_iter.next() {
        if let E::Literal(Literal::Keyword(k)) = key_expr {
            properties.insert(k.0.clone(), val_expr);
        }
    }

    let original_request = properties
        .get("original-request")
        .and_then(|expr| {
            if let E::Literal(Literal::String(s)) = expr {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let goal = properties
        .get("goal")
        .and_then(|expr| {
            if let E::Literal(Literal::String(s)) = expr {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| original_request.clone());

    let mut intent = Intent::new(goal).with_name(name);

    if let Some(expr) = properties.get("constraints") {
        if let Some(m) = map_expr_to_string_value(expr) {
            intent.constraints = m;
        }
    }

    if let Some(expr) = properties.get("preferences") {
        if let Some(m) = map_expr_to_string_value(expr) {
            intent.preferences = m;
        }
    }

    if let Some(expr) = properties.get("success-criteria") {
        let value = expr_to_value(expr);
        intent.success_criteria = Some(value);
    }

    Some(intent)
}

/// Parse LLM response into intent structure using RTFS parser
pub fn parse_llm_intent_response(response: &str) -> Result<Intent, RuntimeError> {
    use rtfs::ast::TopLevel;

    // Extract the first top-level `(intent …)` s-expression from the response
    let intent_block = extract_intent(response).ok_or_else(|| {
        let response_preview = if response.len() > 400 {
            format!("{}...", &response[..400])
        } else {
            response.to_string()
        };
        RuntimeError::Generic(format!(
            "Could not locate a complete (intent …) block in LLM response.\n\n\
            📥 Response preview:\n\
            ┌─────────────────────────────────────────────────────────\n\
            {}\n\
            └─────────────────────────────────────────────────────────\n\n\
            💡 The response should start with (intent \"name\" :goal \"...\" ...)\n\
            Common issues: response is truncated, contains prose before the intent, or missing opening parenthesis.",
            response_preview
        ))
    })?;

    // Sanitize regex literals for parsing
    let sanitized = sanitize_regex_literals(&intent_block);

    // Parse using RTFS parser
    let ast_items = rtfs::parser::parse(&sanitized)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse RTFS intent: {:?}", e)))?;

    // Find the first expression and convert to Intent
    if let Some(TopLevel::Expression(expr)) = ast_items.get(0) {
        intent_from_function_call(&expr).ok_or_else(|| {
            RuntimeError::Generic(
                "Parsed AST expression was not a valid intent definition".to_string(),
            )
        })
    } else {
        Err(RuntimeError::Generic(
            "Parsed AST did not contain a top-level expression for the intent".to_string(),
        ))
    }
}

/// Extract JSON from LLM response, handling common formatting issues
pub fn extract_json_from_response(response: &str) -> String {
    // First, try to find a JSON block enclosed in ```json ... ```
    if let Some(captures) = regex::Regex::new(r"```json\s*([\s\S]*?)\s*```")
        .unwrap()
        .captures(response)
    {
        if let Some(json_block) = captures.get(1) {
            return json_block.as_str().to_string();
        }
    }

    // Fallback to finding the first '{' and last '}'
    if let (Some(start), Some(end)) = (response.find('{'), response.rfind('}')) {
        if start < end {
            return response[start..=end].to_string();
        }
    }

    // If no JSON block is found, return the original response
    response.trim().to_string()
}

/// Parse JSON response as fallback when RTFS parsing fails
pub fn parse_json_intent_response(
    response: &str,
    natural_language: &str,
) -> Result<Intent, RuntimeError> {
    println!("🔄 Attempting to parse response as JSON...");

    // Extract JSON from response (handles markdown code blocks, etc.)
    let json_str = extract_json_from_response(response);

    // Parse the JSON
    let json_value: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
        let json_preview = if json_str.len() > 400 {
            format!(
                "{}...\n[truncated, total length: {} chars]",
                &json_str[..400],
                json_str.len()
            )
        } else {
            json_str.clone()
        };
        RuntimeError::Generic(format!(
            "Failed to parse JSON intent: {}\n\n\
            📥 JSON response preview:\n\
            ┌─────────────────────────────────────────────────────────\n\
            {}\n\
            └─────────────────────────────────────────────────────────\n\n\
            💡 Common JSON issues:\n\
            • Invalid JSON syntax (missing quotes, commas, brackets)\n\
            • Truncated response (incomplete JSON object)\n\
            • Missing required fields (\"goal\" is required)\n\
            • Response contains non-JSON text before/after the JSON",
            e, json_preview
        ))
    })?;

    // Extract intent fields from JSON
    let goal = json_value["goal"]
        .as_str()
        .or_else(|| json_value["Goal"].as_str())
        .or_else(|| json_value["GOAL"].as_str())
        .unwrap_or(natural_language)
        .to_string();

    let name = json_value["name"]
        .as_str()
        .or_else(|| json_value["Name"].as_str())
        .or_else(|| json_value["intent_name"].as_str())
        .map(|s| s.to_string());

    let mut intent = Intent::new(goal)
        .with_name(name.unwrap_or_else(|| format!("intent_{}", uuid::Uuid::new_v4())));

    intent.original_request = natural_language.to_string();

    // Extract constraints if present
    if let Some(constraints_obj) = json_value
        .get("constraints")
        .or_else(|| json_value.get("Constraints"))
    {
        if let Some(obj) = constraints_obj.as_object() {
            for (k, v) in obj {
                let value = match v {
                    serde_json::Value::String(s) => Value::String(s.clone()),
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Value::Integer(i)
                        } else if let Some(f) = n.as_f64() {
                            Value::Float(f)
                        } else {
                            Value::String(v.to_string())
                        }
                    }
                    serde_json::Value::Bool(b) => Value::Boolean(*b),
                    _ => Value::String(v.to_string()),
                };
                intent.constraints.insert(k.clone(), value);
            }
        }
    }

    // Extract preferences if present
    if let Some(preferences_obj) = json_value
        .get("preferences")
        .or_else(|| json_value.get("Preferences"))
    {
        if let Some(obj) = preferences_obj.as_object() {
            for (k, v) in obj {
                let value = match v {
                    serde_json::Value::String(s) => Value::String(s.clone()),
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Value::Integer(i)
                        } else if let Some(f) = n.as_f64() {
                            Value::Float(f)
                        } else {
                            Value::String(v.to_string())
                        }
                    }
                    serde_json::Value::Bool(b) => Value::Boolean(*b),
                    _ => Value::String(v.to_string()),
                };
                intent.preferences.insert(k.clone(), value);
            }
        }
    }

    // Mark that this was parsed from JSON
    intent.metadata.insert(
        "parse_format".to_string(),
        Value::String("json_fallback".to_string()),
    );

    println!("✓ Successfully parsed intent from JSON format");

    Ok(intent)
}

/// A more aggressive implementation that finds all JSON blobs in the response.
pub fn extract_all_json_from_response(response: &str) -> Vec<String> {
    let mut blobs = Vec::new();
    let re = regex::Regex::new(r"\{[\s\S]*?\}").unwrap();
    for cap in re.captures_iter(response) {
        blobs.push(cap[0].to_string());
    }
    blobs
}
