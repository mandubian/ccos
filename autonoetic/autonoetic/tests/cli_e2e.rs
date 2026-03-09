use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn run_autonoetic(args: &[&str], stdin_input: Option<&str>) -> Output {
    run_autonoetic_with_env(args, stdin_input, &[])
}

fn run_autonoetic_with_env(
    args: &[&str],
    stdin_input: Option<&str>,
    envs: &[(&str, &str)],
) -> Output {
    let bin = env!("CARGO_BIN_EXE_autonoetic");
    let mut command = Command::new(bin);
    command.args(args);
    command.envs(envs.iter().copied());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    if stdin_input.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command
        .spawn()
        .expect("autonoetic test process should spawn");
    if let Some(input) = stdin_input {
        child
            .stdin
            .as_mut()
            .expect("stdin pipe should be present")
            .write_all(input.as_bytes())
            .expect("stdin should be writable");
    }
    child
        .wait_with_output()
        .expect("autonoetic test process should complete")
}

struct ChildGuard {
    child: Option<Child>,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl ChildGuard {
    fn stdin_mut(&mut self) -> &mut std::process::ChildStdin {
        self.child
            .as_mut()
            .expect("child should be present")
            .stdin
            .as_mut()
            .expect("stdin pipe should be present")
    }

    fn wait_with_output(&mut self) -> Output {
        self.child
            .take()
            .expect("child should be present")
            .wait_with_output()
            .expect("child should complete")
    }
}

fn spawn_autonoetic(
    args: &[&str],
    envs: &[(&str, &str)],
    stdin_piped: bool,
    capture_output: bool,
) -> ChildGuard {
    let bin = env!("CARGO_BIN_EXE_autonoetic");
    let mut command = Command::new(bin);
    command.args(args);
    command.envs(envs.iter().copied());
    command.stdout(if capture_output {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    command.stderr(if capture_output {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    command.stdin(if stdin_piped {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    ChildGuard {
        child: Some(
            command
                .spawn()
                .expect("autonoetic test process should spawn"),
        ),
    }
}

fn pick_unused_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("port probe should bind")
        .local_addr()
        .expect("port probe should expose local addr")
        .port()
}

fn wait_for_port(addr: SocketAddr, timeout: Duration) {
    let start = Instant::now();
    loop {
        if TcpStream::connect(addr).is_ok() {
            return;
        }
        assert!(start.elapsed() < timeout, "timed out waiting for {}", addr);
        thread::sleep(Duration::from_millis(25));
    }
}

fn write_config(
    config_path: &Path,
    agents_dir: &Path,
    port: u16,
    ofp_port: u16,
    max_pending_spawns_per_agent: usize,
) {
    let yaml = format!(
        "agents_dir: \"{}\"\nport: {}\nofp_port: {}\ntls: false\nmax_pending_spawns_per_agent: {}\nmax_concurrent_spawns: 4\nbackground_scheduler_enabled: false\n",
        agents_dir.display(),
        port,
        ofp_port,
        max_pending_spawns_per_agent
    );
    std::fs::write(config_path, yaml).expect("config should write");
}

fn write_memory_agent(agent_dir: &Path, agent_id: &str) {
    std::fs::create_dir_all(agent_dir).expect("agent dir should create");
    let skill = format!(
        r#"---
name: "{agent_id}"
description: "Terminal chat memory agent"
metadata:
  autonoetic:
    version: "1.0"
    agent:
      id: "{agent_id}"
      name: "{agent_id}"
      description: "Terminal chat memory agent"
    llm_config:
      provider: "openai"
      model: "test-model"
      temperature: 0.0
    capabilities:
      - type: "MemoryWrite"
        scopes: ["*"]
      - type: "MemoryRead"
        scopes: ["*"]
---
# Terminal Memory Agent
Use memory tools when needed.
"#
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill).expect("skill should write");
}

fn write_builder_agent(agent_dir: &Path, agent_id: &str) {
        std::fs::create_dir_all(agent_dir).expect("agent dir should create");
        let skill = [
                "---".to_string(),
                format!("name: \"{}\"", agent_id),
                "description: \"Terminal chat builder agent\"".to_string(),
                "metadata:".to_string(),
                "  autonoetic:".to_string(),
                "    version: \"1.0\"".to_string(),
                "    runtime:".to_string(),
                "      engine: \"autonoetic\"".to_string(),
                "      gateway_version: \"0.1.0\"".to_string(),
                "      sdk_version: \"0.1.0\"".to_string(),
                "      type: \"stateful\"".to_string(),
                "      sandbox: \"bubblewrap\"".to_string(),
                "      runtime_lock: \"runtime.lock\"".to_string(),
                "    agent:".to_string(),
                format!("      id: \"{}\"", agent_id),
                format!("      name: \"{}\"", agent_id),
                "      description: \"Terminal chat builder agent\"".to_string(),
                "    llm_config:".to_string(),
                "      provider: \"openai\"".to_string(),
                "      model: \"test-model\"".to_string(),
                "      temperature: 0.0".to_string(),
                "    capabilities:".to_string(),
                "      - type: \"AgentSpawn\"".to_string(),
                "        max_children: 8".to_string(),
                "---".to_string(),
                "# Terminal Builder Agent".to_string(),
                "Use `agent.install` when the user asks for a durable worker.".to_string(),
                String::new(),
        ]
        .join("\n");
        std::fs::write(agent_dir.join("SKILL.md"), skill).expect("skill should write");
}

fn spawn_openai_stub(captured_bodies: Arc<Mutex<Vec<serde_json::Value>>>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("stub listener should bind");
    let addr = listener
        .local_addr()
        .expect("stub listener should expose addr");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let captured = captured_bodies.clone();
            match stream {
                Ok(mut stream) => {
                    if let Err(err) = handle_stub_connection(&mut stream, captured) {
                        panic!("stub connection failed: {err}");
                    }
                }
                Err(err) => panic!("stub accept failed: {err}"),
            }
        }
    });
    addr
}

fn handle_stub_connection(
    stream: &mut TcpStream,
    captured_bodies: Arc<Mutex<Vec<serde_json::Value>>>,
) -> anyhow::Result<()> {
    let mut header_buf = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        header_buf.push(byte[0]);
        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let headers = String::from_utf8(header_buf)?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("missing Content-Length header"))?;

    let mut body = vec![0_u8; content_length];
    stream.read_exact(&mut body)?;
    let body_json: serde_json::Value = serde_json::from_slice(&body)?;
    captured_bodies.lock().unwrap().push(body_json.clone());

    let latest_user_message = body_json
        .get("messages")
        .and_then(|value| value.as_array())
        .and_then(|messages| {
            messages.iter().rev().find_map(|message| {
                if message.get("role").and_then(|value| value.as_str()) == Some("user") {
                    message
                        .get("content")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                } else {
                    None
                }
            })
        })
        .unwrap_or_default();
    let has_tool_result = body_json
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|messages| {
            messages
                .iter()
                .any(|message| message.get("role").and_then(|value| value.as_str()) == Some("tool"))
        })
        .unwrap_or(false);
    let tool_result_count = body_json
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|messages| {
            messages
                .iter()
                .filter(|message| message.get("role").and_then(|value| value.as_str()) == Some("tool"))
                .count()
        })
        .unwrap_or(0);
    let has_validation_tool_result = body_json
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|messages| {
            messages.iter().any(|message| {
                message.get("role").and_then(|value| value.as_str()) == Some("tool")
                    && message
                        .get("content")
                        .and_then(|value| value.as_str())
                        .map(|content| {
                            content.contains("\"error_type\":\"validation\"")
                                || content.contains("\"error_type\": \"validation\"")
                        })
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if latest_user_message.contains("delay message") {
        thread::sleep(Duration::from_millis(300));
    }

    let response_body = if latest_user_message.contains("repair invalid agent install")
        && !has_tool_result
    {
        let invalid_install_args = serde_json::json!({
            "agent_id": "",
            "name": "repair_worker",
            "description": "Broken worker payload",
            "instructions": "# Broken Worker\nThis payload should fail validation.",
            "files": [
                {
                    "path": "state/seed.txt",
                    "content": "seed"
                }
            ],
            "arm_immediately": false
        });

        serde_json::json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_install_invalid",
                        "type": "function",
                        "function": {
                            "name": "agent.install",
                            "arguments": invalid_install_args.to_string()
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 8}
        })
    } else if latest_user_message.contains("repair invalid agent install")
        && tool_result_count == 1
        && has_validation_tool_result
    {
        let corrected_install_args = serde_json::json!({
            "agent_id": "repair_worker",
            "name": "repair_worker",
            "description": "Worker installed after validation repair.",
            "instructions": "# Repair Worker\nInstalled after a corrected agent.install retry.",
            "files": [
                {
                    "path": "state/seed.txt",
                    "content": "seed"
                }
            ],
            "arm_immediately": false
        });

        serde_json::json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_install_corrected",
                        "type": "function",
                        "function": {
                            "name": "agent.install",
                            "arguments": corrected_install_args.to_string()
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 8}
        })
    } else if latest_user_message.contains("repair invalid agent install") && tool_result_count >= 2 {
        serde_json::json!({
            "choices": [{
                "message": { "content": "Installed repair_worker after retry." },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 8}
        })
    } else if latest_user_message.contains("please store this data")
        && !has_tool_result
    {
        serde_json::json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "memory.write",
                            "arguments": "{\"path\":\"secret.txt\",\"content\":\"secret_value_123\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3}
        })
    } else if latest_user_message.contains("please store this data") && has_tool_result {
        serde_json::json!({
            "choices": [{
                "message": { "content": "I stored it" },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3}
        })
    } else if latest_user_message.contains("what is the data?") && !has_tool_result {
        serde_json::json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_2",
                        "type": "function",
                        "function": {
                            "name": "memory.read",
                            "arguments": "{\"path\":\"secret.txt\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3}
        })
    } else if latest_user_message.contains("what is the data?") && has_tool_result {
        serde_json::json!({
            "choices": [{
                "message": { "content": "The data is secret_value_123" },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3}
        })
    } else if latest_user_message.contains("delay message") {
        serde_json::json!({
            "choices": [{
                "message": { "content": "Delayed reply" },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3}
        })
    } else {
        serde_json::json!({
            "choices": [{
                "message": { "content": "stub assistant reply" },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3}
        })
    };

    let encoded = serde_json::to_vec(&response_body)?;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        encoded.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

#[test]
fn test_agent_init_then_interactive_run_exits_cleanly() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let config_path = temp.path().join("config.yaml");
    let agents_dir = temp.path().join("agents");
    write_config(&config_path, &agents_dir, 4000, 4200, 4);

    let config_arg = config_path.to_string_lossy().to_string();

    let init = run_autonoetic(
        &[
            "--config",
            config_arg.as_str(),
            "agent",
            "init",
            "agent_e2e",
            "--template",
            "coder",
        ],
        None,
    );
    assert!(
        init.status.success(),
        "agent init failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );

    let skill_path = agents_dir.join("agent_e2e").join("SKILL.md");
    let runtime_lock_path = agents_dir.join("agent_e2e").join("runtime.lock");
    assert!(
        skill_path.exists(),
        "SKILL.md should be generated by agent init"
    );
    assert!(
        runtime_lock_path.exists(),
        "runtime.lock should be generated by agent init"
    );

    // Keep the run command hermetic: select a local provider so no API key is required.
    let skill = std::fs::read_to_string(&skill_path).expect("SKILL.md should read");
    let patched = skill.replace("provider: \"openai\"", "provider: \"ollama\"");
    std::fs::write(&skill_path, patched).expect("SKILL.md should update");

    let run = run_autonoetic(
        &[
            "--config",
            config_arg.as_str(),
            "agent",
            "run",
            "agent_e2e",
            "--interactive",
        ],
        Some("/exit\n"),
    );
    assert!(
        run.status.success(),
        "agent run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("Interactive mode enabled. Type /exit to quit."),
        "interactive banner should be printed, got stdout:\n{}",
        stdout
    );
}

#[test]
fn test_terminal_chat_routes_through_gateway_ingress_and_preserves_session() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let config_path = temp.path().join("config.yaml");
    let agents_dir = temp.path().join("agents");
    let agent_id = "memory_chat";
    let jsonrpc_port = pick_unused_port();
    let ofp_port = pick_unused_port();
    write_config(&config_path, &agents_dir, jsonrpc_port, ofp_port, 4);
    write_memory_agent(&agents_dir.join(agent_id), agent_id);

    let captured_bodies = Arc::new(Mutex::new(Vec::new()));
    let stub_addr = spawn_openai_stub(captured_bodies.clone());
    let config_arg = config_path.to_string_lossy().to_string();
    let stub_url = format!("http://{}/v1/chat/completions", stub_addr);
    let gateway_env = [
        ("AUTONOETIC_NODE_ID", "test-gateway"),
        ("AUTONOETIC_NODE_NAME", "Test Gateway"),
        ("AUTONOETIC_SHARED_SECRET", "test-secret"),
        ("AUTONOETIC_LLM_BASE_URL", stub_url.as_str()),
        ("AUTONOETIC_LLM_API_KEY", "test-key"),
    ];
    let gateway_args = ["--config", config_arg.as_str(), "gateway", "start"];
    let _gateway = spawn_autonoetic(&gateway_args, &gateway_env, false, false);
    wait_for_port(
        format!("127.0.0.1:{}", jsonrpc_port)
            .parse()
            .expect("gateway addr should parse"),
        Duration::from_secs(5),
    );

    let session_id = "terminal-session-1";
    let channel_id = "terminal:tester:memory_chat";
    let chat = run_autonoetic(
        &[
            "--config",
            config_arg.as_str(),
            "chat",
            agent_id,
            "--sender-id",
            "tester",
            "--channel-id",
            channel_id,
            "--session-id",
            session_id,
            "--test-mode",
        ],
        Some("please store this data\nwhat is the data?\n/exit\n"),
    );
    assert!(
        chat.status.success(),
        "chat failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&chat.stdout),
        String::from_utf8_lossy(&chat.stderr)
    );
    let stdout = String::from_utf8_lossy(&chat.stdout);
    assert!(
        stdout.contains("I stored it"),
        "expected write reply, got stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("The data is secret_value_123"),
        "expected recall reply, got stdout:\n{}",
        stdout
    );

    let memory_path = agents_dir.join(agent_id).join("state").join("secret.txt");
    assert_eq!(
        std::fs::read_to_string(&memory_path).expect("state should exist"),
        "secret_value_123"
    );

    let gateway_log = std::fs::read_to_string(
        agents_dir
            .join(".gateway")
            .join("history")
            .join("causal_chain.jsonl"),
    )
    .expect("gateway causal log should exist");
    assert!(gateway_log.contains(session_id));
    assert!(gateway_log.contains("\"action\":\"event.ingest.requested\""));
    assert!(gateway_log.contains("\"action\":\"event.ingest.completed\""));

    let agent_log = std::fs::read_to_string(
        agents_dir
            .join(agent_id)
            .join("history")
            .join("causal_chain.jsonl"),
    )
    .expect("agent causal log should exist");
    assert!(agent_log.contains(session_id));
    assert!(agent_log.contains("\"tool_name\":\"memory.write\""));
    assert!(agent_log.contains("\"tool_name\":\"memory.read\""));

    let request_dump = captured_bodies
        .lock()
        .unwrap()
        .iter()
        .map(|body| serde_json::to_string(body).expect("request body should encode"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(request_dump.contains(channel_id));
    assert!(request_dump.contains("sender_id"));
    assert!(request_dump.contains("tester"));
    assert!(request_dump.contains(session_id));
}

#[test]
fn test_terminal_chat_surfaces_gateway_backpressure_errors() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let config_path = temp.path().join("config.yaml");
    let agents_dir = temp.path().join("agents");
    let agent_id = "memory_chat";
    let jsonrpc_port = pick_unused_port();
    let ofp_port = pick_unused_port();
    write_config(&config_path, &agents_dir, jsonrpc_port, ofp_port, 1);
    write_memory_agent(&agents_dir.join(agent_id), agent_id);

    let captured_bodies = Arc::new(Mutex::new(Vec::new()));
    let stub_addr = spawn_openai_stub(captured_bodies.clone());
    let config_arg = config_path.to_string_lossy().to_string();
    let stub_url = format!("http://{}/v1/chat/completions", stub_addr);
    let gateway_env = [
        ("AUTONOETIC_NODE_ID", "test-gateway"),
        ("AUTONOETIC_NODE_NAME", "Test Gateway"),
        ("AUTONOETIC_SHARED_SECRET", "test-secret"),
        ("AUTONOETIC_LLM_BASE_URL", stub_url.as_str()),
        ("AUTONOETIC_LLM_API_KEY", "test-key"),
    ];
    let gateway_args = ["--config", config_arg.as_str(), "gateway", "start"];
    let _gateway = spawn_autonoetic(&gateway_args, &gateway_env, false, false);
    wait_for_port(
        format!("127.0.0.1:{}", jsonrpc_port)
            .parse()
            .expect("gateway addr should parse"),
        Duration::from_secs(5),
    );

    let mut slow_chat = spawn_autonoetic(
        &[
            "--config",
            config_arg.as_str(),
            "chat",
            agent_id,
            "--session-id",
            "terminal-session-slow",
            "--test-mode",
        ],
        &[],
        true,
        true,
    );
    slow_chat
        .stdin_mut()
        .write_all(b"delay message\n/exit\n")
        .expect("slow chat stdin should write");

    // Wait for the slow chat to reach the stub and occupy the gateway's pending execution slot.
    let start = Instant::now();
    while captured_bodies.lock().unwrap().is_empty() {
        if start.elapsed() > Duration::from_secs(5) {
            panic!("Timed out waiting for slow chat to reach LLM stub during backpressure test");
        }
        thread::sleep(Duration::from_millis(10));
    }

    let fast_chat = run_autonoetic(
        &[
            "--config",
            config_arg.as_str(),
            "chat",
            agent_id,
            "--session-id",
            "terminal-session-fast",
            "--test-mode",
        ],
        Some("please store this data\n/exit\n"),
    );
    assert!(
        !fast_chat.status.success(),
        "expected backpressure failure\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&fast_chat.stdout),
        String::from_utf8_lossy(&fast_chat.stderr)
    );
    let stderr = String::from_utf8_lossy(&fast_chat.stderr);
    assert!(
        stderr.contains("pending execution queue is full"),
        "expected gateway backpressure error, got stderr:\n{}",
        stderr
    );

    let slow_output = slow_chat.wait_with_output();
    assert!(
        slow_output.status.success(),
        "slow chat should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&slow_output.stdout),
        String::from_utf8_lossy(&slow_output.stderr)
    );
}

#[test]
fn test_terminal_chat_repairs_invalid_agent_install_in_session() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let config_path = temp.path().join("config.yaml");
    let agents_dir = temp.path().join("agents");
    let agent_id = "builder_chat_repair";
    let jsonrpc_port = pick_unused_port();
    let ofp_port = pick_unused_port();
    write_config(&config_path, &agents_dir, jsonrpc_port, ofp_port, 4);
    write_builder_agent(&agents_dir.join(agent_id), agent_id);

    let captured_bodies = Arc::new(Mutex::new(Vec::new()));
    let stub_addr = spawn_openai_stub(captured_bodies.clone());
    let config_arg = config_path.to_string_lossy().to_string();
    let stub_url = format!("http://{}/v1/chat/completions", stub_addr);
    let gateway_env = [
        ("AUTONOETIC_NODE_ID", "test-gateway"),
        ("AUTONOETIC_NODE_NAME", "Test Gateway"),
        ("AUTONOETIC_SHARED_SECRET", "test-secret"),
        ("AUTONOETIC_LLM_BASE_URL", stub_url.as_str()),
        ("AUTONOETIC_LLM_API_KEY", "test-key"),
    ];
    let gateway_args = ["--config", config_arg.as_str(), "gateway", "start"];
    let _gateway = spawn_autonoetic(&gateway_args, &gateway_env, false, false);
    wait_for_port(
        format!("127.0.0.1:{}", jsonrpc_port)
            .parse()
            .expect("gateway addr should parse"),
        Duration::from_secs(5),
    );

    let chat = run_autonoetic(
        &[
            "--config",
            config_arg.as_str(),
            "chat",
            agent_id,
            "--session-id",
            "terminal-session-repair",
            "--test-mode",
        ],
        Some("repair invalid agent install\n/exit\n"),
    );
    assert!(
        chat.status.success(),
        "chat failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&chat.stdout),
        String::from_utf8_lossy(&chat.stderr)
    );

    let stdout = String::from_utf8_lossy(&chat.stdout);
    assert!(
        stdout.contains("Installed repair_worker after retry."),
        "expected repair completion reply, got stdout:\n{}",
        stdout
    );

    let installed_worker = agents_dir.join("repair_worker");
    assert!(
        installed_worker.join("SKILL.md").exists(),
        "expected repaired install to create child worker SKILL.md"
    );
    assert!(
        installed_worker.join("state").join("seed.txt").exists(),
        "expected repaired install to create declared worker file"
    );

    let captured = captured_bodies.lock().unwrap();
    let saw_validation_feedback = captured.iter().any(|body| {
        body.get("messages")
            .and_then(|value| value.as_array())
            .map(|messages| {
                messages.iter().any(|message| {
                    message.get("role").and_then(|value| value.as_str()) == Some("tool")
                        && message
                            .get("content")
                            .and_then(|value| value.as_str())
                            .map(|content| {
                                content.contains("\"error_type\":\"validation\"")
                                    || content.contains("agent_id must not be empty")
                            })
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });
    let request_dump = captured
        .iter()
        .map(|body| serde_json::to_string(body).expect("request body should encode"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        saw_validation_feedback,
        "expected validation tool_result to be fed back to model, got requests:\n{}",
        request_dump
    );
    assert!(
        request_dump.contains("call_install_invalid"),
        "expected first invalid install tool call"
    );
    assert!(
        request_dump.contains("call_install_corrected"),
        "expected corrected install tool call"
    );
}
