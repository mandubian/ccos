//! Skill Loader
//!
//! Loads skills from URLs (remote or local) and detects format automatically.

use super::parser::{parse_skill_yaml, ParseError};
use super::types::{Skill, SkillOperation};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use serde_json::Value as JsonValue;

/// Error type for skill loading
#[derive(Debug)]
pub enum LoadError {
    /// HTTP request failed
    Network(String),
    /// Parse error
    Parse(ParseError),
    /// Unsupported format
    UnsupportedFormat(String),
    /// Invalid URL
    InvalidUrl(String),
    /// Skill validation failed
    Validation(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Network(msg) => write!(f, "Network error: {}", msg),
            LoadError::Parse(e) => write!(f, "Parse error: {}", e),
            LoadError::UnsupportedFormat(fmt) => write!(f, "Unsupported format: {}", fmt),
            LoadError::InvalidUrl(url) => write!(f, "Invalid URL: {}", url),
            LoadError::Validation(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<ParseError> for LoadError {
    fn from(e: ParseError) -> Self {
        LoadError::Parse(e)
    }
}

/// Skill format detection
#[derive(Debug, Clone, PartialEq)]
pub enum SkillFormat {
    Yaml,
    Markdown,
    Json,
    Unknown,
}

/// Information about a loaded skill
#[derive(Debug, Clone)]
pub struct LoadedSkillInfo {
    /// The parsed skill
    pub skill: Skill,
    /// Source URL
    pub source_url: String,
    /// Detected format
    pub format: SkillFormat,
    /// Capabilities that need registration
    pub capabilities_to_register: Vec<String>,
    /// Whether approval is needed
    pub requires_approval: bool,
    /// Raw content of the skill definition
    pub raw_content: String,
}

/// Load a skill from already-fetched content.
///
/// IMPORTANT: this loader does **not** perform HTTP fetches. Any remote content must be fetched
/// through governed CCOS egress (e.g. `ccos.network.http-fetch`) by the caller.
pub fn load_skill_from_content(source_url: &str, content: &str) -> Result<LoadedSkillInfo, LoadError> {
    // Detect format
    let format = detect_format(source_url, content);

    // Parse based on format
    let skill = match format {
        SkillFormat::Yaml => parse_skill_yaml(content)?,
        SkillFormat::Markdown => parse_skill_markdown(content)?,
        SkillFormat::Json => parse_skill_json(content)?,
        SkillFormat::Unknown => {
            // Try YAML first, then markdown
            parse_skill_yaml(content)
                .or_else(|_| parse_skill_markdown(content))
                .map_err(LoadError::Parse)?
        }
    };

    validate_skill(&skill)?;
    let requires_approval = skill.approval.required || !skill.secrets.is_empty();

    Ok(LoadedSkillInfo {
        capabilities_to_register: skill.capabilities.clone(),
        skill,
        source_url: source_url.to_string(),
        format,
        requires_approval,
        raw_content: content.to_string(),
    })
}

/// Load a skill from a URL.
///
/// For safety, this only supports `file://` URLs. HTTP(S) fetching must be performed by the
/// caller through governed egress. This prevents accidental direct network calls from library code.
pub async fn load_skill_from_url(url: &str) -> Result<LoadedSkillInfo, LoadError> {
    let is_http = url.starts_with("http://") || url.starts_with("https://");
    let is_file = url.starts_with("file://");

    if is_http {
        return Err(LoadError::Network(
            "HTTP(S) skill loading must be performed via governed egress (ccos.network.http-fetch)".to_string(),
        ));
    }
    if !is_file {
        return Err(LoadError::InvalidUrl(url.to_string()));
    }

    let path = url.trim_start_matches("file://");
    log::info!("[SkillLoader] reading file path: {}", path);
    let content = std::fs::read_to_string(path)
        .map_err(|e| LoadError::Network(format!("Failed to read file {}: {}", path, e)))?;

    load_skill_from_content(url, &content)
}

/// Detect skill format from URL extension and content
fn detect_format(url: &str, content: &str) -> SkillFormat {
    // Check URL extension first
    let url_lower = url.to_lowercase();
    if url_lower.ends_with(".yaml") || url_lower.ends_with(".yml") {
        return SkillFormat::Yaml;
    }
    if url_lower.ends_with(".json") {
        return SkillFormat::Json;
    }
    if url_lower.ends_with(".md") {
        return SkillFormat::Markdown;
    }

    // Check content patterns
    let trimmed = content.trim();

    // JSON starts with { or [
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return SkillFormat::Json;
    }

    // Markdown typically starts with # heading
    if trimmed.starts_with('#') {
        return SkillFormat::Markdown;
    }

    // YAML often starts with key: or ---
    if trimmed.starts_with("---") || trimmed.contains(": ") {
        return SkillFormat::Yaml;
    }

    SkillFormat::Unknown
}

/// Validate a skill after parsing
pub fn validate_skill(skill: &Skill) -> Result<(), LoadError> {
    if skill.id.is_empty() {
        return Err(LoadError::Validation("Skill ID is empty".to_string()));
    }
    if skill.operations.is_empty() && skill.capabilities.is_empty() {
        return Err(LoadError::Validation(format!(
            "Skill '{}' has no operations AND no registered capabilities",
            skill.id
        )));
    }
    // We allow skills to have only operations (implicit capabilities) or only capabilities (wrapper).
    Ok(())
}

/// Parse skill from markdown format
///
/// Markdown skills have:
/// - YAML frontmatter (optional, between ---)
/// - Headings describing operations
/// - Code blocks with commands (```bash, ```curl, etc.)
pub fn parse_skill_markdown(content: &str) -> Result<Skill, ParseError> {
    let lines: Vec<&str> = content.lines().collect();

    let mut skill = if content.starts_with("---") {
        // Find end of frontmatter
        if let Some(end_idx) = lines.iter().skip(1).position(|&l| l.trim() == "---") {
            let frontmatter: String = lines[1..=end_idx].join("\n");
            // Try to parse frontmatter as skill YAML
            parse_skill_yaml(&frontmatter)
                .unwrap_or_else(|_| Skill::new("", "", "", Vec::new(), ""))
        } else {
            Skill::new("", "", "", Vec::new(), "")
        }
    } else {
        Skill::new("", "", "", Vec::new(), "")
    };

    // If frontmatter didn't provide basic info, we'll extract it from markdown
    let mut id = skill.id.clone();
    let mut name = skill.name.clone();
    let mut description = skill.description.clone();
    let mut operations = skill.operations.clone();
    let mut instructions = skill.instructions.clone();
    let mut secrets = skill.secrets.clone();
    let onboarding_steps = Vec::new();
    let mut onboarding_raw_content = String::new();

    let mut in_code_block = false;
    let mut code_block_content = String::new();
    let mut current_section = String::new();
    let mut current_section_text = String::new();

    for line in &lines {
        let trimmed = line.trim();

        // Track code blocks
        if trimmed.starts_with("```") {
            if in_code_block {
                // End of code block - extract operation
                let op_name = if current_section.is_empty() {
                    format!("op-{}", operations.len() + 1)
                } else {
                    current_section.clone()
                };
                log::info!(
                    "[SkillLoader] Found potential operation: '{}' content len: {}",
                    op_name,
                    code_block_content.len()
                );

                let input_schema = extract_input_schema_from_code(&code_block_content);
                operations.push(SkillOperation {
                    name: op_name,
                    description: current_section_text.trim().to_string(),
                    endpoint: extract_endpoint_from_curl(&code_block_content),
                    method: extract_method_from_curl(&code_block_content),
                    command: Some(code_block_content.trim().to_string()),
                    runtime: extract_runtime_from_code(&code_block_content),
                    input_schema,
                    output_schema: None,
                });
                code_block_content.clear();
                current_section_text.clear();
            }
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            code_block_content.push_str(line);
            code_block_content.push('\n');
            continue;
        }

        // Parse headings
        if let Some(heading) = trimmed.strip_prefix("# ") {
            name = heading.to_string();
            id = slugify(heading);
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            current_section = slugify(heading);
            current_section_text.clear();
        } else if let Some(heading) = trimmed.strip_prefix("### ") {
            current_section = slugify(heading);
            current_section_text.clear();
        } else if !trimmed.is_empty() {
            // Add to description or instructions based on context
            if current_section.is_empty() && description.is_empty() {
                description = trimmed.to_string();
            } else if current_section == "authentication" || current_section == "secrets" {
                // Extract potential secret names from backticks or uppercase words
                for word in trimmed.split_whitespace() {
                    let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                    if word
                        .chars()
                        .all(|c| c.is_uppercase() || c.is_numeric() || c == '_')
                        && word.len() > 3
                    {
                        if !secrets.contains(&word.to_string()) {
                            secrets.push(word.to_string());
                        }
                    }
                }
                instructions.push_str(trimmed);
                instructions.push('\n');
            } else if current_section == "onboarding" || current_section == "setup" {
                // Capture raw onboarding content for LLM reasoning
                // The agent will read and interpret this prose to determine setup steps
                onboarding_raw_content.push_str(trimmed);
                onboarding_raw_content.push('\n');
                instructions.push_str(trimmed);
                instructions.push('\n');
            } else {
                if trimmed.contains("Base URL:") || trimmed.contains("api_base:") {
                    if let Some(url) = extract_endpoint_from_curl(trimmed) {
                        skill.metadata.insert("api_base".to_string(), url);
                    }
                }
                instructions.push_str(trimmed);
                instructions.push('\n');
                current_section_text.push_str(trimmed);
                current_section_text.push('\n');
            }
        }
    }

    if id.is_empty() {
        id = "unnamed-skill".to_string();
    }
    if name.is_empty() {
        name = "Unnamed Skill".to_string();
    }
    if instructions.is_empty() {
        instructions = description.clone();
    }

    skill.id = id;
    skill.name = name;
    skill.description = description;
    skill.instructions = instructions;
    skill.operations = operations;
    skill.secrets = secrets;

    // Set onboarding config: prefer raw content for LLM reasoning,
    // fall back to structured steps if parsed from YAML/JSON
    if !onboarding_raw_content.is_empty() {
        skill.onboarding = Some(crate::skills::types::OnboardingConfig::from_raw(
            onboarding_raw_content,
        ));
    } else if !onboarding_steps.is_empty() {
        // Backwards compat: if structured steps were parsed (e.g., from YAML frontmatter)
        skill.onboarding = Some(crate::skills::types::OnboardingConfig {
            required: true,
            raw_content: String::new(),
            steps: onboarding_steps,
        });
    }

    // Add capabilities from operations
    for op in &skill.operations {
        if let Some(cap) = extract_capability_from_code(op.command.as_deref().unwrap_or("")) {
            if !skill.capabilities.contains(&cap) {
                skill.capabilities.push(cap);
            }
        }
    }

    Ok(skill)
}

fn extract_endpoint_from_curl(code: &str) -> Option<String> {
    for part in code.split_whitespace() {
        if part.starts_with("http://") || part.starts_with("https://") {
            let url = part.trim_matches('"').trim_matches('\'').to_string();
            log::info!("[SkillLoader] Found endpoint: {}", url);
            return Some(url);
        }
        if part.starts_with('/') {
            log::info!("[SkillLoader] Found relative endpoint: {}", part);
            return Some(part.to_string());
        }
    }
    log::warn!("[SkillLoader] No endpoint found in code block");
    None
}

fn extract_input_schema_from_code(code: &str) -> Option<TypeExpr> {
    let body_json = extract_body_json_from_code(code)?;
    Some(type_expr_from_json_value(&body_json))
}

fn extract_body_json_from_code(code: &str) -> Option<JsonValue> {
    let lines: Vec<&str> = code.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.to_lowercase().starts_with("body:") {
            continue;
        }
        let mut body = trimmed
            .split_once(':')
            .map(|(_, rest)| rest.trim().to_string())
            .unwrap_or_default();
        if body.is_empty() {
            for next in lines.iter().skip(idx + 1) {
                let next_trimmed = next.trim();
                if next_trimmed.is_empty() {
                    break;
                }
                let lower = next_trimmed.to_lowercase();
                if lower.starts_with("returns") || lower.starts_with("headers") {
                    break;
                }
                body.push_str(next_trimmed);
            }
        }
        if body.is_empty() {
            return None;
        }
        if let Ok(json) = serde_json::from_str::<JsonValue>(&body) {
            return Some(json);
        }
    }
    None
}

fn type_expr_from_json_value(value: &JsonValue) -> TypeExpr {
    match value {
        JsonValue::Object(map) => {
            let entries = map
                .iter()
                .map(|(k, v)| MapTypeEntry {
                    key: Keyword(k.to_string()),
                    value_type: Box::new(type_expr_from_json_value(v)),
                    optional: false,
                })
                .collect();
            TypeExpr::Map {
                entries,
                wildcard: None,
            }
        }
        JsonValue::Array(items) => {
            let inner = items
                .get(0)
                .map(type_expr_from_json_value)
                .unwrap_or(TypeExpr::Any);
            TypeExpr::Vector(Box::new(inner))
        }
        JsonValue::String(_) => TypeExpr::Primitive(PrimitiveType::String),
        JsonValue::Number(n) => {
            if n.is_i64() {
                TypeExpr::Primitive(PrimitiveType::Int)
            } else {
                TypeExpr::Primitive(PrimitiveType::Float)
            }
        }
        JsonValue::Bool(_) => TypeExpr::Primitive(PrimitiveType::Bool),
        JsonValue::Null => TypeExpr::Any,
    }
}

fn extract_method_from_curl(code: &str) -> Option<String> {
    let parts: Vec<&str> = code.split_whitespace().collect();
    for i in 0..parts.len() {
        if (parts[i] == "-X" || parts[i] == "--request") && i + 1 < parts.len() {
            return Some(parts[i + 1].to_string());
        }
    }
    if code.starts_with("GET ") {
        return Some("GET".to_string());
    }
    if code.starts_with("POST ") {
        return Some("POST".to_string());
    }
    if code.starts_with("PUT ") {
        return Some("PUT".to_string());
    }
    if code.starts_with("DELETE ") {
        return Some("DELETE".to_string());
    }
    if code.contains("curl ") {
        return Some("GET".to_string());
    }
    None
}

fn extract_runtime_from_code(code: &str) -> Option<String> {
    let trimmed = code.trim();
    if trimmed.starts_with("python") {
        Some("python".to_string())
    } else if trimmed.starts_with("node") {
        Some("node".to_string())
    } else {
        None
    }
}

/// Extract capability ID from code block content
fn extract_capability_from_code(code: &str) -> Option<String> {
    let trimmed = code.trim();

    // curl commands → http-fetch
    if trimmed.starts_with("curl ")
        || trimmed.contains("curl -")
        || trimmed.starts_with("GET /")
        || trimmed.starts_with("POST /")
        || trimmed.starts_with("PUT /")
        || trimmed.starts_with("DELETE /")
    {
        return Some("ccos.network.http-fetch".to_string());
    }

    // Python → sandbox.python
    if trimmed.starts_with("python ") || trimmed.contains("python3 ") {
        return Some("ccos.sandbox.python".to_string());
    }

    // Node → sandbox.node
    if trimmed.starts_with("node ") || trimmed.starts_with("npx ") {
        return Some("ccos.sandbox.node".to_string());
    }

    // jq → json.parse
    if trimmed.starts_with("jq ") {
        return Some("ccos.json.parse".to_string());
    }

    None
}

/// Convert a name to a slug (lowercase, hyphens)
fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Parse skill from JSON format
fn parse_skill_json(content: &str) -> Result<Skill, ParseError> {
    serde_json::from_str(content).map_err(|e| ParseError::Validation(format!("JSON error: {}", e)))
}

// NOTE: Rigid onboarding step parsing removed in favor of freeform LLM reasoning.
// Skills are external data - the agent reads and interprets raw prose to determine setup steps.
// See: OnboardingConfig.raw_content and delegating_engine.rs blueprint injection.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_detection() {
        assert_eq!(
            detect_format("https://example.com/skill.yaml", ""),
            SkillFormat::Yaml
        );
        assert_eq!(
            detect_format("https://example.com/skill.md", ""),
            SkillFormat::Markdown
        );
        assert_eq!(
            detect_format("https://example.com/skill.json", ""),
            SkillFormat::Json
        );
        assert_eq!(
            detect_format("https://example.com/skill", "# My Skill"),
            SkillFormat::Markdown
        );
        assert_eq!(
            detect_format("https://example.com/skill", "{\"id\": \"test\"}"),
            SkillFormat::Json
        );
    }

    #[test]
    fn test_parse_markdown_skill() {
        let markdown = r#"# Moltbook Search

Search for notebooks in Moltbook.

## Usage

Use this skill to find notebooks by keyword.

```bash
curl -X POST https://api.moltbook.com/search \
  -H "Authorization: Bearer $API_KEY" \
  -d '{"query": "machine learning"}'
```
"#;

        let skill = parse_skill_markdown(markdown).unwrap();
        assert_eq!(skill.id, "moltbook-search");
        assert_eq!(skill.name, "Moltbook Search");
        assert!(skill
            .capabilities
            .contains(&"ccos.network.http-fetch".to_string()));
    }

    #[test]
    fn test_parse_markdown_skill_moltbook() {
        let markdown = r#"# Moltbook Agent Skill

## Operations

### Register Agent
```
POST /api/register-agent
Body: { "name": "agent-name", "model": "claude-3" }
Returns: { "agent_id": "...", "secret": "..." }
```

### Human Claim
```
POST /api/human-claim
Headers: Authorization: Bearer {agent_secret}
Body: { "human_x_username": "@human_handle" }
```
"#;

        let skill = parse_skill_markdown(markdown).unwrap();
        assert_eq!(skill.id, "moltbook-agent-skill");
        assert_eq!(skill.operations.len(), 2);
        assert_eq!(skill.operations[0].name, "register-agent");
        assert_eq!(skill.operations[1].name, "human-claim");
        assert!(skill
            .capabilities
            .contains(&"ccos.network.http-fetch".to_string()));
    }

    #[test]
    fn test_parse_markdown_skill_with_onboarding() {
        let markdown = r#"# Moltbook Skill
## Onboarding
1. **register**: Use `register-agent` to get a secret.
2. **verify**: Check state. verify_on_success: (audit.succeeded? "verify-state")
"#;
        let skill = parse_skill_markdown(markdown).unwrap();
        let onboarding = skill.onboarding.unwrap();
        assert_eq!(onboarding.steps.len(), 2);
        assert_eq!(onboarding.steps[0].id, "register");
        assert_eq!(
            onboarding.steps[0].operation,
            Some("register-agent".to_string())
        );
        assert_eq!(onboarding.steps[1].id, "verify");
        assert!(matches!(
            onboarding.steps[1].verify_on_success,
            Some(crate::chat::Predicate::Rtfs(_))
        ));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Search & Find"), "search-find");
        assert_eq!(slugify("API v2.0"), "api-v2-0");
    }

    #[tokio::test]
    async fn test_load_validation_empty() {
        // Skill with no operations
        let skill = Skill::new("test", "Test", "Test", vec!["cap1".to_string()], "inst");
        let result = validate_skill(&skill);
        assert!(result.is_err());
        if let Err(LoadError::Validation(msg)) = result {
            assert!(msg.contains("no operations"));
        } else {
            panic!("Expected validation error");
        }

        // Skill with no capabilities
        let mut skill = Skill::new("test", "Test", "Test", vec![], "inst");
        skill.operations.push(SkillOperation {
            name: "op1".to_string(),
            description: "desc".to_string(),
            endpoint: None,
            method: None,
            command: None,
            runtime: None,
            input_schema: None,
            output_schema: None,
        });
        let result = validate_skill(&skill);
        assert!(result.is_err());
        if let Err(LoadError::Validation(msg)) = result {
            assert!(msg.contains("no registered capabilities"));
        } else {
            panic!("Expected validation error");
        }
    }
}
