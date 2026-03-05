//! Agent directory scanning.

use autonoetic_types::agent::AgentMeta;
use std::path::Path;

/// Scan a directory for agent subdirectories.
///
/// Each subdirectory containing a `SKILL.md` file is treated as an agent.
pub fn scan_agents(dir: &Path) -> anyhow::Result<Vec<AgentMeta>> {
    let mut agents = Vec::new();

    if !dir.exists() {
        tracing::warn!("Agents directory {} does not exist", dir.display());
        return Ok(agents);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                let id = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                agents.push(AgentMeta { id, dir: path });
            }
        }
    }

    tracing::info!("Discovered {} agent(s)", agents.len());
    Ok(agents)
}
