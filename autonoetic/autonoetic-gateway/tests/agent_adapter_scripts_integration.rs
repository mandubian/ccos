use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn script_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("agents")
        .join("evolution")
        .join("agent-adapter.default")
        .join("scripts")
        .join(rel)
}

fn run_python_with_stdin(
    script: &Path,
    args: &[&str],
    stdin_json: &serde_json::Value,
) -> serde_json::Value {
    let mut child = Command::new("python3")
        .arg(script)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("python process should spawn");

    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("stdin should be available");
        stdin
            .write_all(
                serde_json::to_string(stdin_json)
                    .expect("stdin json should serialize")
                    .as_bytes(),
            )
            .expect("stdin should write");
    }

    let output = child.wait_with_output().expect("python should complete");
    assert!(
        output.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("script stdout should be valid json")
}

#[test]
fn test_adapter_scripts_generate_wrapper_with_mapping_hooks() {
    let schema_diff_script = script_path("schema_diff.py");
    let generate_wrapper_script = script_path("generate_wrapper.py");
    assert!(schema_diff_script.exists(), "schema_diff.py should exist");
    assert!(
        generate_wrapper_script.exists(),
        "generate_wrapper.py should exist"
    );

    let diff_input = serde_json::json!({
        "base_accepts": {
            "type": "object",
            "required": ["query"],
            "properties": { "query": { "type": "string" } }
        },
        "base_returns": {
            "type": "object",
            "required": ["summary"],
            "properties": { "summary": { "type": "string" } }
        },
        "target_accepts": {
            "type": "object",
            "required": ["task"],
            "properties": { "task": { "type": "string" } }
        },
        "target_returns": {
            "type": "object",
            "required": ["result"],
            "properties": { "result": { "type": "string" } }
        }
    });
    let diff = run_python_with_stdin(&schema_diff_script, &[], &diff_input);
    assert_eq!(
        diff["requires_input_mapping"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        diff["requires_output_mapping"],
        serde_json::Value::Bool(true)
    );

    let temp = tempfile::tempdir().expect("tempdir should create");
    let base_skill_path = temp.path().join("base.SKILL.md");
    std::fs::write(
        &base_skill_path,
        r#"---
name: "base.agent"
description: "base"
metadata:
  autonoetic:
    version: "1.0"
---
# Base
Base instructions.
"#,
    )
    .expect("base skill should write");

    let target_spec = serde_json::json!({
        "accepts": {
            "type": "object",
            "required": ["task"],
            "properties": { "task": { "type": "string" } }
        },
        "returns": {
            "type": "object",
            "required": ["result"],
            "properties": { "result": { "type": "string" } }
        }
    });

    let output_dir = temp.path().join("wrapper");
    let out = Command::new("python3")
        .arg(&generate_wrapper_script)
        .arg("--base-skill")
        .arg(base_skill_path.to_string_lossy().to_string())
        .arg("--base-agent-id")
        .arg("base.agent")
        .arg("--wrapper-id")
        .arg("base.agent.adapter")
        .arg("--target-spec-json")
        .arg(serde_json::to_string(&target_spec).expect("target spec serializes"))
        .arg("--schema-diff-json")
        .arg(serde_json::to_string(&diff).expect("diff serializes"))
        .arg("--output-dir")
        .arg(output_dir.to_string_lossy().to_string())
        .output()
        .expect("generator should execute");

    assert!(
        out.status.success(),
        "generate_wrapper.py failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let generated: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("generator stdout should be json");
    assert_eq!(generated["wrapper_id"], "base.agent.adapter");
    assert_eq!(
        generated["requires_input_mapping"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        generated["requires_output_mapping"],
        serde_json::Value::Bool(true)
    );

    assert!(output_dir.join("SKILL.md").exists());
    assert!(output_dir.join("scripts").join("pre_map.py").exists());
    assert!(output_dir.join("scripts").join("post_map.py").exists());

    let skill_md = std::fs::read_to_string(output_dir.join("SKILL.md")).expect("SKILL.md readable");
    assert!(
        skill_md.contains("base_agent_id: \"base.agent\""),
        "should have base_agent_id traceability"
    );
    assert!(
        skill_md.contains("generated_at:"),
        "should have generation timestamp"
    );
}

#[test]
fn test_schema_diff_emits_multiple_mappings() {
    let schema_diff_script = script_path("schema_diff.py");
    let diff_input = serde_json::json!({
        "base_accepts": {
            "type": "object",
            "required": ["query", "domain"]
        },
        "base_returns": {
            "type": "object",
            "required": ["summary", "confidence"]
        },
        "target_accepts": {
            "type": "object",
            "required": ["task", "topic"]
        },
        "target_returns": {
            "type": "object",
            "required": ["result", "score"]
        }
    });

    let diff = run_python_with_stdin(&schema_diff_script, &[], &diff_input);
    let input_mappings = diff
        .get("input_mappings")
        .and_then(|v| v.as_array())
        .expect("input_mappings should be array");
    let output_mappings = diff
        .get("output_mappings")
        .and_then(|v| v.as_array())
        .expect("output_mappings should be array");

    assert_eq!(input_mappings.len(), 2);
    assert_eq!(output_mappings.len(), 2);
}
