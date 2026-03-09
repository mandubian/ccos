//! Sandbox runner supporting bubblewrap, docker, and firecracker.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::process::{Child, Command, Stdio};
use std::{fs, io::{BufRead, BufReader, Write}};
use std::os::unix::net::{UnixListener, UnixStream};
use autonoetic_types::causal_chain::EntryStatus;

const DOCKER_IMAGE_ENV: &str = "AUTONOETIC_DOCKER_IMAGE";
const FIRECRACKER_CONFIG_ENV: &str = "AUTONOETIC_FIRECRACKER_CONFIG";
const BWRAP_WORKSPACE_DIR: &str = "/tmp";
const PYTHONPATH_ENV: &str = "PYTHONPATH";
const PYTHON_SDK_PATH_ENV: &str = "AUTONOETIC_PYTHON_SDK_PATH";
const CCOS_SOCKET_ENV: &str = "CCOS_SOCKET_PATH";
const SDK_SOCKET_BASENAME: &str = ".autonoetic_sdk.sock";

struct SdkBridgeGuard {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    socket_path_host: PathBuf,
}

impl Drop for SdkBridgeGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket_path_host);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_file(&self.socket_path_host);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxDriverKind {
    Bubblewrap,
    Docker,
    MicroVm,
}

impl SandboxDriverKind {
    pub fn parse(name: &str) -> anyhow::Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "bubblewrap" | "bwrap" => Ok(Self::Bubblewrap),
            "docker" => Ok(Self::Docker),
            "microvm" | "firecracker" => Ok(Self::MicroVm),
            other => anyhow::bail!("Unsupported sandbox driver '{}'", other),
        }
    }
}

/// Dependency runtime ecosystem used to install generated code dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyRuntime {
    Python,
    NodeJs,
}

/// Thin dependency-install plan applied inside sandbox workspace before execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyPlan {
    pub runtime: DependencyRuntime,
    pub packages: Vec<String>,
}

pub struct SandboxRunner {
    pub process: Child,
    pub driver: SandboxDriverKind,
    _sdk_bridge: Option<SdkBridgeGuard>,
}

impl SandboxRunner {
    /// Spawn with the default bubblewrap driver.
    pub fn spawn(agent_dir: &str, entrypoint: &str) -> anyhow::Result<Self> {
        Self::spawn_with_driver(SandboxDriverKind::Bubblewrap, agent_dir, entrypoint)
    }

    /// Spawn using the manifest-declared driver name.
    pub fn spawn_for_driver(
        driver_name: &str,
        agent_dir: &str,
        entrypoint: &str,
    ) -> anyhow::Result<Self> {
        let driver = SandboxDriverKind::parse(driver_name)?;
        Self::spawn_with_driver(driver, agent_dir, entrypoint)
    }

    /// Spawn using the selected driver and optional dependency install plan.
    pub fn spawn_with_driver(
        driver: SandboxDriverKind,
        agent_dir: &str,
        entrypoint: &str,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_driver_and_dependencies(driver, agent_dir, entrypoint, None)
    }

    /// Spawn with optional dependency management.
    ///
    /// The install phase is executed inside the sandbox workspace with no host-level fallback.
    pub fn spawn_with_driver_and_dependencies(
        driver: SandboxDriverKind,
        agent_dir: &str,
        entrypoint: &str,
        dependencies: Option<&DependencyPlan>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !entrypoint.trim().is_empty(),
            "entrypoint must not be empty"
        );
        if dependencies.is_some() && driver == SandboxDriverKind::MicroVm {
            anyhow::bail!("MicroVM dependency bootstrap is not implemented yet");
        }
        let composed_entrypoint = compose_entrypoint(entrypoint, dependencies)?;
        let (program, args) = match driver {
            SandboxDriverKind::Bubblewrap => {
                if dependencies.is_some() {
                    bubblewrap_shell_command(agent_dir, &composed_entrypoint)?
                } else {
                    bubblewrap_command(agent_dir, entrypoint)?
                }
            }
            SandboxDriverKind::Docker => docker_command(agent_dir, &composed_entrypoint)?,
            SandboxDriverKind::MicroVm => microvm_command(&composed_entrypoint)?,
        };

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut sdk_bridge = None;

        // Expose local Python SDK to bubblewrap workers when available and provide
        // a thin local JSON-RPC bridge for SDK memory/state/event calls.
        if driver == SandboxDriverKind::Bubblewrap {
            if let Some(sdk_path) = resolve_python_sdk_path() {
                inject_pythonpath(&mut command, &sdk_path);
            }
            let bridge = start_sdk_bridge(agent_dir)?;
            command.env(CCOS_SOCKET_ENV, bridge.socket_path_sandbox);
            sdk_bridge = Some(bridge.guard);
        }

        let child = command.spawn()?;
        Ok(Self {
            process: child,
            driver,
            _sdk_bridge: sdk_bridge,
        })
    }
}

struct StartedSdkBridge {
    socket_path_sandbox: String,
    guard: SdkBridgeGuard,
}

fn start_sdk_bridge(agent_dir: &str) -> anyhow::Result<StartedSdkBridge> {
    let host_socket_path = PathBuf::from(agent_dir).join(SDK_SOCKET_BASENAME);
    if host_socket_path.exists() {
        fs::remove_file(&host_socket_path)?;
    }
    let listener = UnixListener::bind(&host_socket_path)?;
    listener.set_nonblocking(true)?;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let agent_dir_buf = PathBuf::from(agent_dir);
    let gateway_dir_buf = gateway_dir_from_agent_dir(&agent_dir_buf)?;

    let handle = thread::spawn(move || {
        run_sdk_bridge_loop(listener, &agent_dir_buf, &gateway_dir_buf, stop_flag);
    });

    Ok(StartedSdkBridge {
        socket_path_sandbox: format!("{}/{}", BWRAP_WORKSPACE_DIR, SDK_SOCKET_BASENAME),
        guard: SdkBridgeGuard {
            stop,
            handle: Some(handle),
            socket_path_host: host_socket_path,
        },
    })
}

fn run_sdk_bridge_loop(
    listener: UnixListener,
    agent_dir: &std::path::Path,
    gateway_dir: &std::path::Path,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(_e) = handle_sdk_client(stream, agent_dir, gateway_dir) {
                    // Ignore bridge client failures in thin compatibility mode.
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_sdk_client(
    mut stream: UnixStream,
    agent_dir: &std::path::Path,
    gateway_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&stream);
        reader.read_line(&mut line)?;
    }
    if line.trim().is_empty() {
        return Ok(());
    }
    let request: serde_json::Value = serde_json::from_str(&line)?;
    let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let params = request
        .get("params")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let response = match dispatch_sdk_method(method, &params, agent_dir, gateway_dir) {
        Ok(result) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        Err(err) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32000,
                "message": err.to_string(),
                "data": {
                    "error_type": "policy_violation"
                }
            }
        }),
    };

    let payload = serde_json::to_string(&response)? + "\n";
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn validate_sdk_relative_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!path.trim().is_empty(), "path must not be empty");
    anyhow::ensure!(!path.starts_with('/'), "absolute paths are not allowed");
    anyhow::ensure!(
        !path.split('/').any(|part| part == ".." || part.is_empty() || part == "."),
        "path traversal is not allowed"
    );
    Ok(())
}

fn gateway_dir_from_agent_dir(agent_dir: &std::path::Path) -> anyhow::Result<PathBuf> {
    let agents_root = agent_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("agent directory is missing agents-root parent"))?;
    let gateway_dir = agents_root.join(".gateway");
    fs::create_dir_all(&gateway_dir)?;
    Ok(gateway_dir)
}

fn agent_id_from_agent_dir(agent_dir: &std::path::Path) -> anyhow::Result<String> {
    let id = agent_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("unable to derive agent id from agent directory"))?;
    Ok(id.to_string())
}

fn next_sdk_event_seq(log_path: &std::path::Path) -> anyhow::Result<u64> {
    if !log_path.exists() {
        return Ok(1);
    }
    let entries = crate::causal_chain::CausalLogger::read_entries(log_path)?;
    Ok(entries.last().map(|e| e.event_seq + 1).unwrap_or(1))
}

fn log_sdk_memory_event(
    agent_dir: &std::path::Path,
    action: &str,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let actor_id = agent_id_from_agent_dir(agent_dir)?;
    let log_path = agent_dir.join("history").join("causal_chain.jsonl");
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let logger = crate::causal_chain::CausalLogger::new(&log_path)?;
    let event_seq = next_sdk_event_seq(&log_path)?;
    logger.log(
        &actor_id,
        "sdk-bridge",
        None,
        event_seq,
        "memory",
        action,
        EntryStatus::Success,
        Some(payload),
    )
}

fn load_json_file(path: &std::path::Path) -> anyhow::Result<serde_json::Value> {
    if !path.exists() {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    let body = fs::read_to_string(path)?;
    let parsed: serde_json::Value = serde_json::from_str(&body)?;
    Ok(parsed)
}

fn write_json_file(path: &std::path::Path, value: &serde_json::Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn list_state_keys(state_dir: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    if !state_dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(state_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            out.push(format!("state/{}", entry.file_name().to_string_lossy()));
        }
    }
    out.sort();
    Ok(out)
}

fn dispatch_sdk_method(
    method: &str,
    params: &serde_json::Map<String, serde_json::Value>,
    agent_dir: &std::path::Path,
    gateway_dir: &std::path::Path,
) -> anyhow::Result<serde_json::Value> {
    match method {
        "memory.read" => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("memory.read requires path"))?;
            validate_sdk_relative_path(path)?;
            let content = fs::read_to_string(agent_dir.join(path))?;
            Ok(serde_json::json!({ "content": content }))
        }
        "memory.write" => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("memory.write requires path"))?;
            validate_sdk_relative_path(path)?;
            let content = params
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("memory.write requires content"))?;
            let target = agent_dir.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(target, content)?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "memory.list_keys" => {
            let keys = list_state_keys(&agent_dir.join("state"))?;
            Ok(serde_json::json!({ "keys": keys }))
        }
        "memory.remember" => {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("memory.remember requires key"))?;
            let value = params
                .get("value")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("memory.remember requires value"))?;
            let scope = params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("sdk");
            let agent_id = agent_id_from_agent_dir(agent_dir)?;
            let mem = crate::runtime::memory::Tier2Memory::new(gateway_dir, &agent_id)?;
            let source_ref = format!("sdk_bridge:{}", agent_id);
            let content = serde_json::to_string(&value)?;
            let memory = mem.remember(key, scope, &agent_id, &source_ref, &content)?;
            let _ = log_sdk_memory_event(
                agent_dir,
                "remember",
                serde_json::json!({
                    "memory_id": memory.memory_id,
                    "scope": memory.scope,
                    "source_ref": memory.source_ref,
                }),
            );
            Ok(serde_json::json!({
                "ok": true,
                "memory_id": memory.memory_id,
                "scope": memory.scope,
                "source_ref": memory.source_ref,
            }))
        }
        "memory.recall" => {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("memory.recall requires key"))?;
            let agent_id = agent_id_from_agent_dir(agent_dir)?;
            let mem = crate::runtime::memory::Tier2Memory::new(gateway_dir, &agent_id)?;
            match mem.recall(key) {
                Ok(memory) => {
                    let parsed = serde_json::from_str::<serde_json::Value>(&memory.content)
                        .unwrap_or_else(|_| serde_json::Value::String(memory.content.clone()));
                    let _ = log_sdk_memory_event(
                        agent_dir,
                        "recall",
                        serde_json::json!({
                            "memory_id": memory.memory_id,
                            "scope": memory.scope,
                            "source_ref": memory.source_ref,
                        }),
                    );
                    Ok(serde_json::json!({
                        "value": parsed,
                        "scope": memory.scope,
                        "source_ref": memory.source_ref,
                    }))
                }
                Err(_) => Ok(serde_json::json!({ "value": serde_json::Value::Null })),
            }
        }
        "memory.search" => {
            let query = params
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("memory.search requires query"))?
                .to_ascii_lowercase();
            let scope = params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("sdk");
            let agent_id = agent_id_from_agent_dir(agent_dir)?;
            let mem = crate::runtime::memory::Tier2Memory::new(gateway_dir, &agent_id)?;
            let mut results = Vec::<String>::new();
            for memory in mem.search(scope, None)? {
                let hay = format!("{} {}", memory.memory_id, memory.content).to_ascii_lowercase();
                if hay.contains(&query) {
                    results.push(format!("{}: {}", memory.memory_id, memory.content));
                }
            }
            let _ = log_sdk_memory_event(
                agent_dir,
                "search",
                serde_json::json!({
                    "scope": scope,
                    "query": query,
                    "count": results.len(),
                }),
            );
            Ok(serde_json::json!({ "results": results }))
        }
        "state.checkpoint" => {
            let data = params
                .get("data")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("state.checkpoint requires data"))?;
            let checkpoint = serde_json::json!({ "data": data });
            write_json_file(&agent_dir.join("state").join("sdk_checkpoint.json"), &checkpoint)?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "state.get_checkpoint" => {
            let path = agent_dir.join("state").join("sdk_checkpoint.json");
            let payload = load_json_file(&path)?;
            Ok(serde_json::json!({ "data": payload.get("data").cloned().unwrap_or(serde_json::Value::Null) }))
        }
        "events.emit" => {
            let event_type = params
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("events.emit requires type"))?;
            let data = params
                .get("data")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let events_path = agent_dir.join("history").join("sdk_events.jsonl");
            if let Some(parent) = events_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let event = serde_json::json!({ "type": event_type, "data": data });
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(events_path)?;
            writeln!(file, "{}", serde_json::to_string(&event)?)?;
            Ok(serde_json::json!({ "ok": true }))
        }
        other => anyhow::bail!("unsupported SDK method '{}'", other),
    }
}

fn resolve_python_sdk_path() -> Option<String> {
    if let Ok(path) = std::env::var(PYTHON_SDK_PATH_ENV) {
        if !path.trim().is_empty() {
            return Some(path);
        }
    }

    let local: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("autonoetic-sdk")
        .join("python");
    if local.exists() {
        return Some(local.to_string_lossy().to_string());
    }

    None
}

fn inject_pythonpath(command: &mut Command, sdk_path: &str) {
    match std::env::var(PYTHONPATH_ENV) {
        Ok(existing) if !existing.trim().is_empty() => {
            command.env(PYTHONPATH_ENV, format!("{}:{}", sdk_path, existing));
        }
        _ => {
            command.env(PYTHONPATH_ENV, sdk_path);
        }
    }
}

fn split_entrypoint(entrypoint: &str) -> anyhow::Result<(String, Vec<String>)> {
    let parts: Vec<&str> = entrypoint.split_whitespace().collect();
    anyhow::ensure!(!parts.is_empty(), "entrypoint must not be empty");
    let program = parts[0].to_string();
    let args = parts[1..].iter().map(|s| s.to_string()).collect();
    Ok((program, args))
}

fn bubblewrap_command(agent_dir: &str, entrypoint: &str) -> anyhow::Result<(String, Vec<String>)> {
    let (program, args) = split_entrypoint(entrypoint)?;
    let mut argv = vec![
        "--ro-bind".to_string(),
        "/".to_string(),
        "/".to_string(),
        "--bind".to_string(),
        agent_dir.to_string(),
        BWRAP_WORKSPACE_DIR.to_string(),
        "--chdir".to_string(),
        BWRAP_WORKSPACE_DIR.to_string(),
        "--unshare-all".to_string(),
        "--".to_string(),
        program,
    ];
    argv.extend(args);
    Ok(("bwrap".to_string(), argv))
}

fn bubblewrap_shell_command(
    agent_dir: &str,
    shell_command: &str,
) -> anyhow::Result<(String, Vec<String>)> {
    anyhow::ensure!(
        !shell_command.trim().is_empty(),
        "shell command must not be empty"
    );
    let argv = vec![
        "--ro-bind".to_string(),
        "/".to_string(),
        "/".to_string(),
        "--bind".to_string(),
        agent_dir.to_string(),
        BWRAP_WORKSPACE_DIR.to_string(),
        "--chdir".to_string(),
        BWRAP_WORKSPACE_DIR.to_string(),
        "--unshare-all".to_string(),
        "--".to_string(),
        "sh".to_string(),
        "-lc".to_string(),
        shell_command.to_string(),
    ];
    Ok(("bwrap".to_string(), argv))
}

fn docker_command(agent_dir: &str, entrypoint: &str) -> anyhow::Result<(String, Vec<String>)> {
    let image = std::env::var(DOCKER_IMAGE_ENV).map_err(|_| {
        anyhow::anyhow!("Missing required environment variable {}", DOCKER_IMAGE_ENV)
    })?;
    let argv = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--network".to_string(),
        "none".to_string(),
        "--volume".to_string(),
        format!("{}:/workspace", agent_dir),
        "--workdir".to_string(),
        "/workspace".to_string(),
        image,
        "sh".to_string(),
        "-lc".to_string(),
        entrypoint.to_string(),
    ];
    Ok(("docker".to_string(), argv))
}

fn microvm_command(_entrypoint: &str) -> anyhow::Result<(String, Vec<String>)> {
    let cfg = std::env::var(FIRECRACKER_CONFIG_ENV).map_err(|_| {
        anyhow::anyhow!(
            "Missing required environment variable {}",
            FIRECRACKER_CONFIG_ENV
        )
    })?;
    let argv = vec!["--config-file".to_string(), cfg];
    Ok(("firecracker".to_string(), argv))
}

fn compose_entrypoint(entrypoint: &str, deps: Option<&DependencyPlan>) -> anyhow::Result<String> {
    let Some(plan) = deps else {
        return Ok(entrypoint.to_string());
    };
    anyhow::ensure!(
        !plan.packages.is_empty(),
        "dependency plan must contain at least one package"
    );
    for pkg in &plan.packages {
        validate_dependency_package(pkg)?;
    }
    let joined = plan.packages.join(" ");
    let composed = match plan.runtime {
        DependencyRuntime::Python => format!(
            "python3 -m venv .autonoetic_venv && ./.autonoetic_venv/bin/pip install --disable-pip-version-check --no-input --no-cache-dir {joined} && {entrypoint}"
        ),
        DependencyRuntime::NodeJs => format!(
            "npm install --no-save --prefix .autonoetic_node {joined} && NODE_PATH=.autonoetic_node/node_modules {entrypoint}"
        ),
    };
    Ok(composed)
}

fn validate_dependency_package(pkg: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !pkg.trim().is_empty(),
        "dependency package name must not be empty"
    );
    // Keep package token grammar tight to avoid shell injection in thin bootstrap strings.
    let allowed = pkg.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '.' | '_' | '-' | '<' | '>' | '=' | '!' | '~' | '[' | ']' | ',' | '@' | '/'
            )
    });
    anyhow::ensure!(allowed, "invalid dependency token '{}'", pkg);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_driver_kind() {
        assert_eq!(
            SandboxDriverKind::parse("bubblewrap").expect("bubblewrap should parse"),
            SandboxDriverKind::Bubblewrap
        );
        assert_eq!(
            SandboxDriverKind::parse("docker").expect("docker should parse"),
            SandboxDriverKind::Docker
        );
        assert_eq!(
            SandboxDriverKind::parse("microvm").expect("microvm should parse"),
            SandboxDriverKind::MicroVm
        );
    }

    #[test]
    fn test_bubblewrap_command_shape() {
        let (_bin, argv) = bubblewrap_command("/tmp/agent", "python main.py")
            .expect("bubblewrap command should build");
        assert_eq!(argv[0], "--ro-bind");
        assert_eq!(argv[3], "--bind");
        assert_eq!(argv[4], "/tmp/agent");
        assert_eq!(argv[5], "/tmp");
        assert_eq!(argv[6], "--chdir");
        assert_eq!(argv[7], "/tmp");
        assert_eq!(argv[9], "--");
        assert_eq!(argv[10], "python");
        assert_eq!(argv[11], "main.py");
    }

    #[test]
    fn test_docker_command_requires_env() {
        let old = std::env::var(DOCKER_IMAGE_ENV).ok();
        std::env::remove_var(DOCKER_IMAGE_ENV);
        let err = docker_command("/tmp/agent", "python main.py")
            .expect_err("docker command should fail without env");
        assert!(
            err.to_string().contains(DOCKER_IMAGE_ENV),
            "error should mention missing docker env"
        );
        if let Some(v) = old {
            std::env::set_var(DOCKER_IMAGE_ENV, v);
        }
    }

    #[test]
    fn test_microvm_command_requires_env() {
        let old = std::env::var(FIRECRACKER_CONFIG_ENV).ok();
        std::env::remove_var(FIRECRACKER_CONFIG_ENV);
        let err = microvm_command("ignored").expect_err("microvm command should fail without env");
        assert!(
            err.to_string().contains(FIRECRACKER_CONFIG_ENV),
            "error should mention missing firecracker env"
        );
        if let Some(v) = old {
            std::env::set_var(FIRECRACKER_CONFIG_ENV, v);
        }
    }

    #[test]
    fn test_compose_python_dependencies() {
        let plan = DependencyPlan {
            runtime: DependencyRuntime::Python,
            packages: vec!["requests==2.32.3".to_string()],
        };
        let cmd =
            compose_entrypoint("python main.py", Some(&plan)).expect("compose should succeed");
        assert!(cmd.contains("python3 -m venv .autonoetic_venv"));
        assert!(cmd.contains("pip install"));
        assert!(cmd.contains("requests==2.32.3"));
        assert!(cmd.ends_with("python main.py"));
    }

    #[test]
    fn test_compose_node_dependencies() {
        let plan = DependencyPlan {
            runtime: DependencyRuntime::NodeJs,
            packages: vec!["lodash@4.17.21".to_string()],
        };
        let cmd = compose_entrypoint("node app.js", Some(&plan)).expect("compose should succeed");
        assert!(cmd.contains("npm install --no-save --prefix .autonoetic_node"));
        assert!(cmd.contains("NODE_PATH=.autonoetic_node/node_modules"));
        assert!(cmd.ends_with("node app.js"));
    }

    #[test]
    fn test_dependency_token_validation_rejects_unsafe_chars() {
        let err =
            validate_dependency_package("foo;rm -rf /").expect_err("unsafe token should fail");
        assert!(err.to_string().contains("invalid dependency token"));
    }

    #[test]
    fn test_bubblewrap_shell_command_shape() {
        let (_bin, argv) =
            bubblewrap_shell_command("/tmp/agent", "echo hi").expect("shell command should build");
        assert_eq!(argv[0], "--ro-bind");
        assert_eq!(argv[3], "--bind");
        assert_eq!(argv[4], "/tmp/agent");
        assert_eq!(argv[5], "/tmp");
        assert_eq!(argv[6], "--chdir");
        assert_eq!(argv[7], "/tmp");
        assert_eq!(argv[9], "--");
        assert_eq!(argv[10], "sh");
        assert_eq!(argv[11], "-lc");
        assert_eq!(argv[12], "echo hi");
    }

    #[test]
    fn test_sdk_dispatch_memory_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let gateway_dir = gateway_dir_from_agent_dir(temp.path()).expect("gateway dir should resolve");
        let params = serde_json::Map::from_iter(vec![
            ("key".to_string(), json!("skills.worker.latest")),
            ("value".to_string(), json!({"n": 13})),
        ]);
        let remember = dispatch_sdk_method("memory.remember", &params, temp.path(), &gateway_dir)
            .expect("remember should succeed");
        assert_eq!(remember["ok"], json!(true));

        let recall_params = serde_json::Map::from_iter(vec![("key".to_string(), json!("skills.worker.latest"))]);
        let recall = dispatch_sdk_method("memory.recall", &recall_params, temp.path(), &gateway_dir)
            .expect("recall should succeed");
        assert_eq!(recall["value"]["n"], json!(13));
    }

    #[test]
    fn test_sdk_dispatch_checkpoint_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let gateway_dir = gateway_dir_from_agent_dir(temp.path()).expect("gateway dir should resolve");
        let checkpoint_params =
            serde_json::Map::from_iter(vec![("data".to_string(), json!({"cursor": 42}))]);
        let written = dispatch_sdk_method("state.checkpoint", &checkpoint_params, temp.path(), &gateway_dir)
            .expect("checkpoint should succeed");
        assert_eq!(written["ok"], json!(true));

        let loaded = dispatch_sdk_method(
            "state.get_checkpoint",
            &serde_json::Map::new(),
            temp.path(),
            &gateway_dir,
        )
            .expect("load checkpoint should succeed");
        assert_eq!(loaded["data"]["cursor"], json!(42));
    }
}
