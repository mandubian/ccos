use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rtfs::runtime::error::RuntimeError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptConfig {
    pub intent_prompt_id: String,
    pub intent_prompt_version: String,
    pub plan_prompt_id: String,
    pub plan_prompt_version: String,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            intent_prompt_id: "intent_generation".to_string(),
            intent_prompt_version: "v1".to_string(),
            plan_prompt_id: "plan_generation".to_string(),
            plan_prompt_version: "v1".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PromptTemplate {
    pub id: String,
    pub version: String,
    pub sections: Vec<(String, String)>, // (name, content)
}

pub trait PromptStore: Send + Sync {
    fn get_template(&self, id: &str, version: &str) -> Result<PromptTemplate, RuntimeError>;
}

#[derive(Clone)]
pub struct FilePromptStore {
    base_dir: PathBuf,
}

impl FilePromptStore {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    fn read_section(&self, id: &str, version: &str, name: &str) -> Result<String, RuntimeError> {
        let path = self
            .base_dir
            .join(id)
            .join(version)
            .join(format!("{}.md", name));
        fs::read_to_string(&path).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read prompt section {} for {}/{}: {}",
                name, id, version, e
            ))
        })
    }
}

impl PromptStore for FilePromptStore {
    fn get_template(&self, id: &str, version: &str) -> Result<PromptTemplate, RuntimeError> {
        // Default section set
        let section_names = vec!["grammar", "strategy", "few_shots", "anti_patterns", "task"];
        let mut sections = Vec::new();
        for name in section_names {
            if let Ok(content) = self.read_section(id, version, name) {
                sections.push((name.to_string(), content));
            }
        }
        if sections.is_empty() {
            return Err(RuntimeError::Generic(format!(
                "No prompt sections found for {}/{} in {}",
                id,
                version,
                self.base_dir.display()
            )));
        }
        Ok(PromptTemplate {
            id: id.to_string(),
            version: version.to_string(),
            sections,
        })
    }
}

#[derive(Clone)]
pub struct PromptManager<S: PromptStore> {
    store: S,
}

impl<S: PromptStore> PromptManager<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn render(
        &self,
        id: &str,
        version: &str,
        vars: &HashMap<String, String>,
    ) -> Result<String, RuntimeError> {
        let template = self.store.get_template(id, version)?;
        let mut buf = String::new();
        for (_name, content) in template.sections {
            buf.push_str(&content);
            if !buf.ends_with('\n') {
                buf.push('\n');
            }
            buf.push('\n');
        }
        // simple variable substitution: {var}
        let mut rendered = buf;
        for (k, v) in vars {
            let needle = format!("{{{}}}", k);
            rendered = rendered.replace(&needle, v);
        }
        Ok(rendered)
    }
}
