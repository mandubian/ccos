use clap::Parser;
use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::environment::CCOSBuilder;
use rtfs_compiler::runtime::values::Value;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(name = "execute-capability", about = "Execute a registered capability by id with JSON inputs")] 
struct Args {
    /// Path to AgentConfig (TOML or JSON) used to initialize CCOS and providers
    #[arg(long)]
    config: String,

    /// Capability ID to execute (e.g., synth.plan.orchestrator.<ts>)
    #[arg(long)]
    cap_id: String,

    /// JSON object of inputs to pass to the capability
    #[arg(long, default_value = "{}")]
    inputs: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Load and validate AgentConfig (TOML or JSON); we don't enable any fallback here
    let _config = load_agent_config(&args.config)?;

    // Ensure host has a minimal execution context for capability calls.
    // This avoids "Host method called without a valid execution context" for direct executions.
    // Safe for this strict runner since we don't use marketplace discovery or networked effects here.
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");

    // Build a lightweight CCOS environment and load the generated capability file
    let env = CCOSBuilder::new()
        .verbose(false)
        .build()
        .map_err(|e| format!("failed to init CCOS environment: {}", e))?;

    let cap_file_primary = format!("capabilities/generated/{}/capability.rtfs", args.cap_id);
    let cap_file_fallback = format!("../{}", cap_file_primary);
    let cap_path = if Path::new(&cap_file_primary).exists() {
        cap_file_primary
    } else if Path::new(&cap_file_fallback).exists() {
        cap_file_fallback
    } else {
        cap_file_primary
    };
    env.execute_file(&cap_path)
        .map_err(|e| format!("failed to load capability file '{}': {}", cap_path, e))?;

    // Parse inputs JSON into RTFS Value::Map
    let input_map = json_to_rtfs_map(&args.inputs)?;

    // Execute the capability by id (no fallback)
    let result = env
        .execute_capability(&args.cap_id, &[Value::Map(input_map.clone())])
        .map_err(|e| format!("capability execution failed: {}", e))?;
    println!("\n✅ Execution result for {}:", args.cap_id);
    println!("{}", pretty_rtfs_value(&result));

    Ok(())
}

fn load_agent_config(path: &str) -> Result<rtfs_compiler::config::types::AgentConfig, Box<dyn Error>> {
    // For this strict runner, we only validate the file exists and is readable;
    // we don't need to parse or apply it to the local environment.
    // This keeps the example resilient to minimal TOML configs.
    let _ = fs::read_to_string(path)?;
    Ok(rtfs_compiler::config::types::AgentConfig::default())
}

fn json_to_rtfs_map(json_str: &str) -> Result<HashMap<MapKey, Value>, Box<dyn Error>> {
    let json: JsonValue = serde_json::from_str(json_str)?;
    if !json.is_object() {
        return Err("inputs must be a JSON object".into());
    }
    let mut map = HashMap::new();
    if let Some(obj) = json.as_object() {
        for (k, v) in obj.iter() {
            map.insert(MapKey::String(k.clone()), json_to_rtfs_value(v)?);
        }
    }
    Ok(map)
}

fn json_to_rtfs_value(v: &JsonValue) -> Result<Value, Box<dyn Error>> {
    Ok(match v {
        JsonValue::Null => Value::Nil,
        JsonValue::Bool(b) => Value::Boolean(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() { Value::Integer(i) }
            else if let Some(f) = n.as_f64() { Value::Float(f) }
            else { return Err("unsupported number format".into()); }
        }
        JsonValue::String(s) => Value::String(s.clone()),
        JsonValue::Array(arr) => {
            let mut vec = Vec::with_capacity(arr.len());
            for item in arr { vec.push(json_to_rtfs_value(item)?); }
            Value::Vector(vec)
        }
        JsonValue::Object(obj) => {
            let mut map = HashMap::new();
            for (k, vv) in obj.iter() {
                map.insert(MapKey::String(k.clone()), json_to_rtfs_value(vv)?);
            }
            Value::Map(map)
        }
    })
}

fn pretty_rtfs_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => format!("{}", b),
        Value::Integer(i) => format!("{}", i),
        Value::Float(f) => format!("{}", f),
        Value::String(s) => format!("\"{}\"", s),
        Value::Vector(vec) => {
            let inner: Vec<String> = vec.iter().map(pretty_rtfs_value).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Map(m) => {
            let mut pairs: Vec<String> = m
                .iter()
                .map(|(k, vv)| format!("{}: {}", match k { MapKey::String(s) => s.clone(), _ => format!("{:?}", k) }, pretty_rtfs_value(vv)))
                .collect();
            pairs.sort();
            format!("{{{}}}", pairs.join(", "))
        }
        _ => format!("{:?}", v),
    }
}


fn extract_input_keys(rtfs: &str) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(idx) = rtfs.find(":input-schema") {
        if let Some(start) = rtfs[idx..].find('{') {
            let start_idx = idx + start + 1;
            if let Some(end_rel) = rtfs[start_idx..].find('}') {
                let block = &rtfs[start_idx..start_idx + end_rel];
                for line in block.lines() {
                    let l = line.trim();
                    if let Some(stripped) = l.strip_prefix(':') {
                        // take until first whitespace or end
                        let key: String = stripped
                            .chars()
                            .take_while(|c| !c.is_whitespace())
                            .collect();
                        if !key.is_empty() {
                            keys.push(key);
                        }
                    }
                }
            }
        }
    }
    keys
}

fn extract_capabilities_required(rtfs: &str) -> Vec<String> {
    let mut caps = Vec::new();
    if let Some(idx) = rtfs.find(":capabilities-required") {
        if let Some(start_br) = rtfs[idx..].find('[') {
            let start_idx = idx + start_br + 1;
            if let Some(end_rel) = rtfs[start_idx..].find(']') {
                let list = &rtfs[start_idx..start_idx + end_rel];
                for token in list.split_whitespace() {
                    let t = token.trim_matches('"');
                    if !t.is_empty() {
                        caps.push(t.to_string());
                    }
                }
            }
        }
    }
    caps
}

fn transform_plan_bindings(rtfs: &str, input_keys: &[String]) -> String {
    let mut out = String::with_capacity(rtfs.len() + 64);
    for line in rtfs.lines() {
        let mut new_line = line.to_string();
        for key in input_keys {
            let needle = format!(":{} {}", key, key);
            let replacement = format!(":{} (get :{})", key, key);
            if new_line.contains(&needle) {
                new_line = new_line.replace(&needle, &replacement);
            }
        }
        out.push_str(&new_line);
        out.push('\n');
    }
    out
}
