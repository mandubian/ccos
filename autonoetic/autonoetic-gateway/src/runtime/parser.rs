//! SKILL.md Parser.

use autonoetic_types::agent::AgentManifest;
use gray_matter::{engine::YAML, Matter};

/// Parser for `SKILL.md` files.
pub struct SkillParser;

impl SkillParser {
    /// Parses a `SKILL.md` content string into an `AgentManifest` and the Markdown body.
    pub fn parse(content: &str) -> anyhow::Result<(AgentManifest, String)> {
        let matter = Matter::<YAML>::new();
        let parsed = matter
            .parse(content)
            .map_err(|e| anyhow::anyhow!("gray_matter error: {}", e))?;

        let data: gray_matter::Pod = parsed
            .data
            .ok_or_else(|| anyhow::anyhow!("No YAML frontmatter found in SKILL.md"))?;

        let manifest = data.deserialize::<AgentManifest>()?;

        Ok((manifest, parsed.content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_skill() {
        let content = r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "uv.lock"
agent:
  id: "test_agent"
  name: "Test Agent"
  description: "A test agent"
---
# Test Agent Instructions
Here are the instructions.
"#;
        let (manifest, body) = SkillParser::parse(content).unwrap();
        assert_eq!(manifest.version, "1.0");
        assert_eq!(manifest.agent.id, "test_agent");
        assert_eq!(
            body.trim(),
            "# Test Agent Instructions\nHere are the instructions."
        );
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# Just markdown\nNo frontmatter here.";
        assert!(SkillParser::parse(content).is_err());
    }
}
