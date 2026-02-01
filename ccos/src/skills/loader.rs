//! Skill Loader
//!
//! Loads skills from URLs (remote or local) and detects format automatically.

use super::parser::{parse_skill_yaml, ParseError};
use super::types::{Skill, SkillOperation};

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
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Network(msg) => write!(f, "Network error: {}", msg),
            LoadError::Parse(e) => write!(f, "Parse error: {}", e),
            LoadError::UnsupportedFormat(fmt) => write!(f, "Unsupported format: {}", fmt),
            LoadError::InvalidUrl(url) => write!(f, "Invalid URL: {}", url),
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
}

/// Load a skill from a URL
pub async fn load_skill_from_url(url: &str) -> Result<LoadedSkillInfo, LoadError> {
    // Basic URL validation (defense-in-depth): only allow http(s) sources for now.
    // This keeps skill loading consistent with the spec's security posture.
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(LoadError::InvalidUrl(url.to_string()));
    }

    // Fetch content
    let content = fetch_url_content(url).await?;

    // Detect format
    let format = detect_format(url, &content);

    // Parse based on format
    let skill = match format {
        SkillFormat::Yaml => parse_skill_yaml(&content)?,
        SkillFormat::Markdown => parse_skill_markdown(&content)?,
        SkillFormat::Json => parse_skill_json(&content)?,
        SkillFormat::Unknown => {
            // Try YAML first, then markdown
            parse_skill_yaml(&content)
                .or_else(|_| parse_skill_markdown(&content))
                .map_err(|e| LoadError::Parse(e))?
        }
    };

    let requires_approval = skill.approval.required || !skill.secrets.is_empty();

    Ok(LoadedSkillInfo {
        capabilities_to_register: skill.capabilities.clone(),
        skill,
        source_url: url.to_string(),
        format,
        requires_approval,
    })
}

/// Fetch content from URL using reqwest
async fn fetch_url_content(url: &str) -> Result<String, LoadError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| LoadError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(url)
        .header("User-Agent", "CCOS-SkillLoader/1.0")
        .send()
        .await
        .map_err(|e| LoadError::Network(format!("Request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(LoadError::Network(format!(
            "HTTP {} for {}",
            response.status(),
            url
        )));
    }

    response
        .text()
        .await
        .map_err(|e| LoadError::Network(format!("Failed to read response: {}", e)))
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

/// Parse skill from markdown format
///
/// Markdown skills have:
/// - YAML frontmatter (optional, between ---)
/// - Headings describing operations
/// - Code blocks with commands (```bash, ```curl, etc.)
pub fn parse_skill_markdown(content: &str) -> Result<Skill, ParseError> {
    let lines: Vec<&str> = content.lines().collect();

    // Check for YAML frontmatter
    if content.starts_with("---") {
        // Find end of frontmatter
        if let Some(end_idx) = lines.iter().skip(1).position(|&l| l.trim() == "---") {
            let frontmatter: String = lines[1..=end_idx].join("\n");
            // Try to parse frontmatter as skill YAML
            if let Ok(skill) = parse_skill_yaml(&frontmatter) {
                return Ok(skill);
            }
        }
    }

    // Extract skill info from markdown structure
    let mut id = String::new();
    let mut name = String::new();
    let mut description = String::new();
    let mut operations = Vec::new();
    let mut instructions = String::new();
    let mut secrets = Vec::new();

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

                operations.push(SkillOperation {
                    name: op_name,
                    description: current_section_text.trim().to_string(),
                    endpoint: extract_endpoint_from_curl(&code_block_content),
                    method: extract_method_from_curl(&code_block_content),
                    command: Some(code_block_content.trim().to_string()),
                    runtime: extract_runtime_from_code(&code_block_content),
                    input_schema: None,
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
            } else {
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

    let mut skill = Skill::new(id, name, description, Vec::new(), instructions);
    skill.operations = operations;

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
            return Some(part.trim_matches('"').trim_matches('\'').to_string());
        }
        if part.starts_with('/') {
            return Some(part.to_string());
        }
    }
    None
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
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Search & Find"), "search-find");
        assert_eq!(slugify("API v2.0"), "api-v2-0");
    }
}
