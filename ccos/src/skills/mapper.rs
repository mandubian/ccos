//! Skill Mapper
//!
//! Maps skills to capabilities and executes skill intents.

use crate::capability_marketplace::types::CapabilityManifest;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::skills::types::Skill;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Error type for skill operations
#[derive(Debug)]
pub enum SkillError {
    /// Skill not found
    NotFound(String),
    /// Capability not available for skill
    CapabilityNotFound(String),
    /// Capability not approved
    NotApproved(String),
    /// Execution error
    Execution(String),
    /// Validation error
    Validation(String),
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillError::NotFound(id) => write!(f, "Skill not found: {}", id),
            SkillError::CapabilityNotFound(id) => write!(f, "Capability not found: {}", id),
            SkillError::NotApproved(id) => write!(f, "Skill not approved: {}", id),
            SkillError::Execution(msg) => write!(f, "Execution error: {}", msg),
            SkillError::Validation(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for SkillError {}

/// Intent representing what the user wants to do
#[derive(Debug, Clone)]
pub struct Intent {
    /// Natural language description of the intent
    pub description: String,
    /// Extracted parameters (if any)
    pub params: HashMap<String, Value>,
    /// Context from conversation
    pub context: HashMap<String, String>,
}

impl Intent {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            params: HashMap::new(),
            context: HashMap::new(),
        }
    }

    pub fn with_param(mut self, key: impl Into<String>, value: Value) -> Self {
        self.params.insert(key.into(), value);
        self
    }
}

/// Maps skills to capabilities and executes skill intents
pub struct SkillMapper {
    /// Registered skills by ID
    skills: HashMap<String, Skill>,
    /// Capability marketplace for resolving capabilities
    marketplace: Arc<CapabilityMarketplace>,
}

impl SkillMapper {
    /// Create a new skill mapper
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self {
            skills: HashMap::new(),
            marketplace,
        }
    }

    /// Register a skill
    pub fn register_skill(&mut self, skill: Skill) {
        self.skills.insert(skill.id.clone(), skill);
    }

    /// Register multiple skills
    pub fn register_skills(&mut self, skills: Vec<Skill>) {
        for skill in skills {
            self.register_skill(skill);
        }
    }

    /// Get a skill by ID
    pub fn get_skill(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id)
    }

    /// List all registered skills
    pub fn list_skills(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// List visible skills (for UI)
    pub fn list_visible_skills(&self) -> Vec<&Skill> {
        self.skills.values().filter(|s| s.display.visible).collect()
    }

    /// List skills by category
    pub fn list_skills_by_category(&self, category: &str) -> Vec<&Skill> {
        self.skills
            .values()
            .filter(|s| s.display.category == category)
            .collect()
    }

    /// Resolve capabilities for a skill
    /// Returns the capability manifests required by the skill
    pub async fn resolve_capabilities(
        &self,
        skill: &Skill,
    ) -> Result<Vec<CapabilityManifest>, SkillError> {
        let mut manifests = Vec::new();

        for cap_id in &skill.capabilities {
            if self.marketplace.has_capability(cap_id).await {
                if let Some(manifest) = self.marketplace.get_capability(cap_id).await {
                    manifests.push(manifest);
                } else {
                    return Err(SkillError::CapabilityNotFound(cap_id.clone()));
                }
            } else {
                return Err(SkillError::CapabilityNotFound(cap_id.clone()));
            }
        }

        Ok(manifests)
    }

    /// Check if a skill's capabilities are all available
    pub async fn is_skill_available(&self, skill_id: &str) -> bool {
        if let Some(skill) = self.skills.get(skill_id) {
            for cap_id in &skill.capabilities {
                if !self.marketplace.has_capability(cap_id).await {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    /// Execute a skill with the given intent
    /// This is a simplified version - full implementation would use LLM for intent interpretation
    pub async fn execute_skill_intent(
        &self,
        skill_id: &str,
        intent: &Intent,
    ) -> Result<Value, SkillError> {
        let skill = self
            .skills
            .get(skill_id)
            .ok_or_else(|| SkillError::NotFound(skill_id.to_string()))?;

        // Resolve capabilities to ensure they're available
        let _capabilities = self.resolve_capabilities(skill).await?;

        // For now, return a simple acknowledgment
        // Full implementation would:
        // 1. Use LLM to interpret intent with skill instructions
        // 2. Select appropriate capability
        // 3. Route through GovernanceKernel for execution
        // 4. Return result

        // This is a stub - real implementation needs LLM integration
        let result = rtfs::ast::MapKey::String("result".to_string());
        let mut map = std::collections::HashMap::new();
        map.insert(
            result,
            Value::String(format!(
                "Skill '{}' would process intent: {}",
                skill.name, intent.description
            )),
        );
        map.insert(
            rtfs::ast::MapKey::String("skill_id".to_string()),
            Value::String(skill_id.to_string()),
        );
        map.insert(
            rtfs::ast::MapKey::String("capabilities".to_string()),
            Value::List(
                skill
                    .capabilities
                    .iter()
                    .map(|c| Value::String(c.clone()))
                    .collect(),
            ),
        );

        Ok(Value::Map(map))
    }

    /// Generate a prompt for LLM skill interpretation
    /// This can be used with an external LLM to interpret user intent
    pub fn generate_interpretation_prompt(&self, skill: &Skill, user_input: &str) -> String {
        let mut prompt = format!(
            "You are executing the skill: {}\n\nDescription: {}\n\n",
            skill.name, skill.description
        );

        prompt.push_str("Instructions:\n");
        prompt.push_str(&skill.instructions);
        prompt.push_str("\n\n");

        if !skill.examples.is_empty() {
            prompt.push_str("Examples:\n");
            for example in &skill.examples {
                prompt.push_str(&format!(
                    "- Input: \"{}\"\n  Capability: {}\n  Params: {}\n",
                    example.input, example.capability, example.params
                ));
            }
            prompt.push_str("\n");
        }

        prompt.push_str(&format!(
            "Available capabilities: {}\n\n",
            skill.capabilities.join(", ")
        ));

        prompt.push_str(&format!("User input: \"{}\"\n\n", user_input));
        prompt.push_str("Respond with the capability to call and the parameters in JSON format.\n");

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::registry::CapabilityRegistry;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_skill_registration() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let mut mapper = SkillMapper::new(marketplace);

        let skill = Skill::new(
            "test-skill",
            "Test Skill",
            "A test skill",
            vec!["test.cap".to_string()],
            "Test instructions",
        );

        mapper.register_skill(skill);
        assert!(mapper.get_skill("test-skill").is_some());
        assert_eq!(mapper.list_skills().len(), 1);
    }

    #[test]
    fn test_intent_builder() {
        let intent = Intent::new("Find coffee shops near me")
            .with_param("location", Value::String("current".to_string()));

        assert_eq!(intent.description, "Find coffee shops near me");
        assert!(intent.params.contains_key("location"));
    }

    #[tokio::test]
    async fn test_generate_prompt() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let mapper = SkillMapper::new(marketplace);

        let skill = Skill::new(
            "search-places",
            "Search Places",
            "Find nearby places",
            vec!["maps.search".to_string()],
            "Use this to find restaurants and shops.",
        );

        let prompt = mapper.generate_interpretation_prompt(&skill, "Find pizza near me");
        assert!(prompt.contains("Search Places"));
        assert!(prompt.contains("Find pizza near me"));
        assert!(prompt.contains("maps.search"));
    }
}
