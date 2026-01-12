use ccos::approval::queue::{
    ApprovalAuthority, ApprovalQueue, ApprovedDiscovery, HasId, HasName, ServerInfo,
};
use ccos::approval::DiscoverySource;
use ccos::utils::fs::get_workspace_root;
use chrono::Utc;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct LegacyManifest {
    pub source: serde_json::Value,
    pub server_info: ServerInfo,
    pub capability_files: Option<Vec<String>>,
    pub api_info: Option<serde_json::Value>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting migration of discovery artifacts to RTFS...");

    let workspace_root = get_workspace_root();
    let queue = ApprovalQueue::new(workspace_root.clone());

    // Access approved directory directly
    let approved_dir = workspace_root.join("capabilities/servers/approved");
    if !approved_dir.exists() {
        println!(
            "No approved capability directory found at {}",
            approved_dir.display()
        );
        return Ok(());
    }

    let entries = fs::read_dir(&approved_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let server_json = path.join("server.json");
            if server_json.exists() {
                println!(
                    "Processing server: {}",
                    path.file_name().unwrap().to_string_lossy()
                );

                // Try to load as ApprovedDiscovery (modern)
                let content = fs::read_to_string(&server_json)?;
                let approved_discovery: Option<ApprovedDiscovery> =
                    serde_json::from_str::<ApprovedDiscovery>(&content).ok();

                let to_save = if let Some(mut existing) = approved_discovery {
                    println!("  Matches current schema. Re-saving as RTFS...");
                    existing
                } else {
                    // Start legacy processing
                    println!("  Does not match current schema. Trying legacy...");
                    let legacy: Option<LegacyManifest> =
                        serde_json::from_str::<LegacyManifest>(&content).ok();

                    if let Some(l) = legacy {
                        println!("  Found legacy manifest. converting...");
                        // Create ID from directory name
                        let id = path.file_name().unwrap().to_string_lossy().to_string();

                        let source = if let Some(type_val) = l.source.get("type") {
                            if type_val == "OpenAPI" {
                                let url = l
                                    .source
                                    .get("spec_url")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                DiscoverySource::WebSearch { url }
                            } else {
                                serde_json::from_value(l.source.clone())
                                    .unwrap_or(DiscoverySource::LocalConfig)
                            }
                        } else {
                            serde_json::from_value(l.source.clone())
                                .unwrap_or(DiscoverySource::LocalConfig)
                        };

                        ApprovedDiscovery {
                            id,
                            source,
                            server_info: l.server_info,
                            domain_match: false,
                            risk_assessment: None,
                            requesting_goal: None,
                            approved_at: Utc::now(),
                            approved_by: ApprovalAuthority::Auto,
                            approval_reason: Some("Migrated from legacy format".to_string()),
                            capability_files: l.capability_files,
                            version: 1,
                            last_successful_call: None,
                            consecutive_failures: 0,
                            total_calls: 0,
                            total_errors: 0,
                        }
                    } else {
                        println!("  ERROR: Could not parse as LegacyManifest. Skipping.");
                        continue;
                    }
                };

                // Save using updated logic (writes server.rtfs, removes server.json)
                // Use public storage methods directly or queue helper
                // queue.save_to_dir is cleaner but requires path
                // queue.save_to_dir(&path.parent().unwrap(), &to_save)?; // Saves INTO dir/name

                // Approved path is `capabilities/servers/approved`.
                // `save_to_dir` appends `name` (sanitized).
                // `path` is `.../approved/Cat_Facts_Spec`.
                // `queue.save_to_dir` expects BASE dir.

                // Note: `save_to_dir` sanitizes name. If `id` != `name`, dir name might change?
                // Legacy logic didn't enforce name=dirname.
                // But generally safe_name(name) is used.
                // `l.server_info.name` for Cat_Facts_Spec is "Cat Facts Spec". Sanitize -> "cat_facts_spec"?
                // Existing dir is "Cat_Facts_Spec".
                // If sanitize changes it, we get a NEW directory.
                // If so, we should remove the old one?
                // Or rename?

                // Let's assume we want to keep using `save_to_dir` logic to ensure consistency.
                // If it creates a new directory, fine. We can manually cleanup old if we want, or just verify afterwards.

                queue
                    .save_to_dir(&approved_dir, &to_save)
                    .map_err(|e| format!("{}", e))?;
                println!("  Saved successfully.");
            }
        }
    }

    println!("Migration complete.");
    Ok(())
}
