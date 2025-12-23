
use std::path::{PathBuf};

fn get_workspace_root() -> PathBuf {
    // Simulate what's happening in ccos_explore
    // 1. Initial get_workspace_root() -> current_dir
    // 2. load_agent_config calls set_workspace_root(current_dir/config)
    
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("config")
}

fn resolve_workspace_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        get_workspace_root().join(p)
    }
}

fn test_load_core_capabilities() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    println!("Current dir: {:?}", cwd);

    let config_path = cwd.join("config/agent_config.toml");
    println!("Config path: {:?}", config_path);

    let mut caps_dir_from_config = String::new();

    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            for line in content.lines() {
                if line.trim().starts_with("capabilities_dir") {
                    if let Some(val) = line.split('=').nth(1) {
                        caps_dir_from_config = val.trim().trim_matches('"').trim_matches('\'').to_string();
                        println!("Found capabilities_dir in config: {}", caps_dir_from_config);
                        break;
                    }
                }
            }
        }
    }

    if caps_dir_from_config.is_empty() {
        caps_dir_from_config = "../capabilities".to_string();
    }

    let resolved_caps_dir = resolve_workspace_path(&caps_dir_from_config);
    println!("Resolved capabilities dir: {:?}", resolved_caps_dir);
    
    let core_dir = resolved_caps_dir.join("core");
    println!("Core dir: {:?}", core_dir);

    if core_dir.exists() && core_dir.is_dir() {
        println!("✅ Core dir exists.");
        if let Ok(entries) = std::fs::read_dir(&core_dir) {
            for entry in entries.flatten() {
                println!("  - Found: {:?}", entry.path());
            }
        }
    } else {
        println!("❌ Core dir DOES NOT exist!");
    }
    
    // Test ApprovalQueue path logic
    let approval_path = get_workspace_root().join("capabilities/servers/approved");
    println!("Approval path (as expected by MCPDiscoveryService): {:?}", approval_path);
    if approval_path.exists() {
        println!("✅ Approval path exists.");
    } else {
        println!("❌ Approval path DOES NOT exist!");
    }
}

fn main() {
    test_load_core_capabilities();
}
