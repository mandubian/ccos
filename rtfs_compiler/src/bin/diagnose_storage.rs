use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Allow overriding root via CLI arg or ENV; default to ./demo_storage relative to CWD
    let arg_root = std::env::args().nth(1);
    let storage_root = arg_root
        .map(PathBuf::from)
        .or_else(|| std::env::var("CCOS_STORAGE_ROOT").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("demo_storage"));
    let graph_dir = storage_root.join("intents");
    let plans_dir = storage_root.join("plans");

    println!("🔍 Scanning storage root: {:?}", storage_root);

    // --- Intents ---
    let mut intents = Vec::new();
    let graph_index = graph_dir.join("index.json");
    if graph_index.exists() {
        let content = fs::read_to_string(&graph_index).expect("Failed to read graph index");
        let index: serde_json::Map<String, Value> =
            serde_json::from_str(&content).expect("Invalid graph index JSON");
        for (hash, rel_path) in index.iter() {
            let node_path = graph_dir.join(rel_path.as_str().unwrap_or(""));
            if node_path.exists() {
                let node_content = fs::read_to_string(&node_path).unwrap_or_default();
                if let Ok(node_json) = serde_json::from_str::<Value>(&node_content) {
                    if let Some(intent_id) = node_json.get("intent_id") {
                        intents.push((intent_id.as_str().unwrap_or("").to_string(), hash.clone()));
                    }
                }
            }
        }
    }
    println!("Found {} intents:", intents.len());
    for (intent_id, hash) in &intents {
        println!("  - intent_id: {} (hash: {})", intent_id, hash);
    }

    // --- Plans --- (support new indices plan_index.json + intent_index.json for fast path)
    let mut plans = Vec::new(); // (plan_id, intent_ids, hash)
    let fast_plan_index = plans_dir.join("plan_index.json");
    let fast_intent_index = plans_dir.join("intent_index.json");
    let mut fast_plan_map: Option<HashMap<String, String>> = None; // plan_id -> hash
    let mut fast_intent_map: Option<HashMap<String, Vec<String>>> = None; // intent_id -> [hash]
    if fast_plan_index.exists() && fast_intent_index.exists() {
        if let Ok(s) = fs::read_to_string(&fast_plan_index) {
            if let Ok(m) = serde_json::from_str(&s) {
                fast_plan_map = Some(m);
            }
        }
        if let Ok(s) = fs::read_to_string(&fast_intent_index) {
            if let Ok(m) = serde_json::from_str(&s) {
                fast_intent_map = Some(m);
            }
        }
    }
    if let Some(plan_map) = &fast_plan_map {
        // Need to open each hash file to extract intent_ids (could optimize by persisting map, but fine now)
        for (plan_id, hash) in plan_map.iter() {
            // Attempt deterministic shard path first (aa/bb/hash.json)
            let shard_path = if hash.len() >= 4 {
                Some(
                    plans_dir
                        .join(&hash[0..2])
                        .join(&hash[2..4])
                        .join(format!("{}.json", hash)),
                )
            } else {
                None
            };
            let mut found_path = shard_path.filter(|p| p.exists());
            if found_path.is_none() && plans_dir.exists() {
                // Fallback: manual stack-based DFS (avoid external crate)
                let mut stack = vec![plans_dir.clone()];
                while let Some(dir) = stack.pop() {
                    if let Ok(read) = fs::read_dir(&dir) {
                        for entry in read.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                stack.push(path);
                                continue;
                            }
                            if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                                if fname == format!("{}.json", hash) {
                                    found_path = Some(path.clone());
                                    break;
                                }
                            }
                        }
                    }
                    if found_path.is_some() {
                        break;
                    }
                }
            }
            if let Some(p) = found_path {
                if let Ok(plan_content) = fs::read_to_string(&p) {
                    if let Ok(plan_json) = serde_json::from_str::<Value>(&plan_content) {
                        let intent_ids = plan_json
                            .get("intent_ids")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        plans.push((plan_id.clone(), intent_ids, hash.clone()));
                    }
                }
            }
        }
    } else {
        // Legacy path: use archive's index.json
        let plans_index = plans_dir.join("index.json");
        if plans_index.exists() {
            if let Ok(content) = fs::read_to_string(&plans_index) {
                if let Ok(index) = serde_json::from_str::<serde_json::Map<String, Value>>(&content)
                {
                    for (hash, rel_path) in index.iter() {
                        let plan_path = plans_dir.join(rel_path.as_str().unwrap_or(""));
                        if plan_path.exists() {
                            if let Ok(plan_content) = fs::read_to_string(&plan_path) {
                                if let Ok(plan_json) = serde_json::from_str::<Value>(&plan_content)
                                {
                                    let intent_ids =
                                        plan_json.get("intent_ids").and_then(|v| v.as_array());
                                    let plan_id = plan_json
                                        .get("plan_id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    plans.push((
                                        plan_id.to_string(),
                                        intent_ids
                                            .map(|ids| {
                                                ids.iter()
                                                    .filter_map(|id| id.as_str())
                                                    .map(|s| s.to_string())
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default(),
                                        hash.clone(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    println!("\nFound {} plans:", plans.len());
    for (plan_id, intent_ids, hash) in &plans {
        println!(
            "  - plan_id: {} (hash: {}) intents: {:?}",
            plan_id, hash, intent_ids
        );
    }

    // --- Linkage ---
    println!("\nIntent → Plan linkage:");
    if let Some(intent_map) = &fast_intent_map {
        for (iid, hashes) in intent_map.iter() {
            // map each hash back to plan_id (reverse lookup plan_map) if available
            if let Some(plan_map) = &fast_plan_map {
                let mut pid_list = Vec::new();
                for (pid, h) in plan_map.iter() {
                    if hashes.contains(h) {
                        pid_list.push(pid.clone());
                    }
                }
                if pid_list.is_empty() {
                    println!("  intent_id: {} → [NO PLAN FOUND]", iid);
                } else {
                    println!("  intent_id: {} → {:?}", iid, pid_list);
                }
            }
        }
    } else {
        for (intent_id, _) in &intents {
            let mut found = false;
            for (plan_id, intent_ids, _) in &plans {
                if intent_ids.contains(intent_id) {
                    println!("  intent_id: {} → plan_id: {}", intent_id, plan_id);
                    found = true;
                }
            }
            if !found {
                println!("  intent_id: {} → [NO PLAN FOUND]", intent_id);
            }
        }
    }
    println!(
        "\nSummary: {} intents, {} plans",
        intents.len(),
        plans.len()
    );
}
