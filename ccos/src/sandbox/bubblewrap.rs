//! Bubblewrap sandbox runtime for Python code execution
//!
//! Uses bubblewrap (bwrap) for process isolation on Linux.
//! Mounts Python interpreter and executes code in an isolated environment.

use rtfs::runtime::error::RuntimeError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::sandbox::config::SandboxConfig;

/// Input file specification
#[derive(Debug, Clone)]
pub struct InputFile {
    /// File name as it appears in sandbox
    pub name: String,
    /// Host path to file
    pub host_path: PathBuf,
}

/// Result of sandbox execution for Python code
#[derive(Debug, Clone)]
pub struct SandboxExecutionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code (None if killed by signal)
    pub exit_code: Option<i32>,
    /// Output files generated (name -> content bytes)
    pub output_files: HashMap<String, Vec<u8>>,
}

/// Dispatcher closure used by [`BubblewrapSandbox::execute_python_interactive`].
///
/// Receives `(capability_id, json_inputs)` and returns a JSON-serialised result.
/// The implementation in `chat/mod.rs` maps this to `marketplace.execute_capability`.
pub type CapabilityDispatcher = std::sync::Arc<
    dyn Fn(
            String,
            serde_json::Value,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<serde_json::Value, RuntimeError>> + Send>,
        > + Send
        + Sync,
>;

/// Content of ccos_sdk.py embedded at compile time for mounting into the sandbox.
const CCOS_SDK_PY: &str = include_str!("ccos_sdk.py");

/// Bubblewrap sandbox runtime for Python execution
pub struct BubblewrapSandbox {
    scanner: SecurityScanner,
}

/// Env var to run ccos.execute.* without bubblewrap (when user namespaces are unavailable).
/// Less secure; use only when bwrap fails with "Creating new namespace failed".
pub const CCOS_EXECUTE_NO_SANDBOX: &str = "CCOS_EXECUTE_NO_SANDBOX";
pub const CCOS_SANDBOX_MAX_NPROC: &str = "CCOS_SANDBOX_MAX_NPROC";

pub fn no_sandbox_requested() -> bool {
    std::env::var(CCOS_EXECUTE_NO_SANDBOX)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v == "yes")
        .unwrap_or(false)
}

fn sandbox_max_nproc() -> Option<u64> {
    std::env::var(CCOS_SANDBOX_MAX_NPROC)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
}

impl BubblewrapSandbox {
    /// Create a new bubblewrap sandbox.
    /// When CCOS_EXECUTE_NO_SANDBOX=1, bwrap is not required (execution will run unjailed).
    pub fn new() -> Result<Self, RuntimeError> {
        if !no_sandbox_requested() && !Self::is_bwrap_available() {
            return Err(RuntimeError::Generic(
                "bubblewrap not found. Install with: sudo apt install bubblewrap".to_string(),
            ));
        }

        let scanner = SecurityScanner::new(&[
            r"subprocess\.".to_string(),
            r"os\.system\(".to_string(),
            r"exec\(".to_string(),
            r"eval\(".to_string(),
            r"__import__\(".to_string(),
            r"pickle\.loads?".to_string(),
        ])
        .map_err(|e| RuntimeError::Generic(format!("Security scanner error: {}", e)))?;

        Ok(Self { scanner })
    }

    /// Check if bwrap is available
    fn is_bwrap_available() -> bool {
        std::process::Command::new("which")
            .arg("bwrap")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Execute Python code with file mounting and optional dependency installation
    /// Execute Python code with file mounting and optional dependency installation
    pub async fn execute_python(
        &self,
        code: &str,
        input_files: &[InputFile],
        config: &SandboxConfig,
        dependencies: Option<&[String]>,
        dep_manager: Option<&super::DependencyManager>,
    ) -> Result<SandboxExecutionResult, RuntimeError> {
        self.execute_with_runtime(
            "python",
            code,
            input_files,
            config,
            dependencies,
            dep_manager,
        )
        .await
    }

    /// Execute JavaScript code (Node.js) with file mounting and optional dependency installation
    pub async fn execute_javascript(
        &self,
        code: &str,
        input_files: &[InputFile],
        config: &SandboxConfig,
        dependencies: Option<&[String]>,
        dep_manager: Option<&super::DependencyManager>,
    ) -> Result<SandboxExecutionResult, RuntimeError> {
        self.execute_with_runtime(
            "javascript",
            code,
            input_files,
            config,
            dependencies,
            dep_manager,
        )
        .await
    }

    /// Execute Python code with a two-way CCOS_CALL:: / CCOS_RESULT:: IPC protocol.
    ///
    /// The sandbox mounts `ccos_sdk.py` at `/workspace/input/ccos_sdk.py` and sets
    /// `PYTHONPATH=/workspace/input` so scripts can `import ccos_sdk`.
    ///
    /// When Python code calls `ccos_sdk.memory.get(...)` (or any other SDK method):
    /// 1. Python prints `CCOS_CALL::<json>` to stdout and blocks on `sys.stdin.readline()`.
    /// 2. This method intercepts the line, calls `dispatcher(cap_id, json_inputs)`.
    /// 3. Writes `CCOS_RESULT::<json>` to the process's stdin.
    /// 4. Python unblocks and receives the result value.
    ///
    /// Non-CCOS lines are accumulated as visible stdout returned to the caller.
    pub async fn execute_python_interactive(
        &self,
        code: &str,
        input_files: &[InputFile],
        config: &SandboxConfig,
        dispatcher: CapabilityDispatcher,
    ) -> Result<SandboxExecutionResult, RuntimeError> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        self.scanner
            .scan(code)
            .map_err(|e| RuntimeError::Generic(format!("Security violation: {}", e)))?;

        // Create temp workspace
        let work_dir = tempfile::tempdir()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create temp dir: {}", e)))?;
        let input_dir = work_dir.path().join("input");
        let output_dir = work_dir.path().join("output");
        let workspace_dir = work_dir.path().join("workspace");

        fs::create_dir_all(&input_dir)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to create input dir: {}", e)))?;
        fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to create output dir: {}", e)))?;
        fs::create_dir_all(&workspace_dir)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to create workspace dir: {}", e)))?;

        // Write SDK to input_dir so it gets mounted at /workspace/input/ccos_sdk.py
        let sdk_host_path = input_dir.join("ccos_sdk.py");
        fs::write(&sdk_host_path, CCOS_SDK_PY)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to write ccos_sdk.py: {}", e)))?;

        let timeout_ms = config
            .resources
            .as_ref()
            .map(|r| {
                if r.timeout_ms > 0 {
                    r.timeout_ms
                } else {
                    30000
                }
            })
            .unwrap_or(30000);

        // Build command — bwrap or unjailed
        let mut cmd = if no_sandbox_requested() {
            log::warn!(
                "[sandbox] CCOS_EXECUTE_NO_SANDBOX set; running Python without bubblewrap (less secure)"
            );
            // Copy input files into input_dir
            for file in input_files {
                if file.host_path.exists() {
                    let dest = input_dir.join(&file.name);
                    fs::copy(&file.host_path, &dest).await.map_err(|e| {
                        RuntimeError::Generic(format!(
                            "Failed to copy input file {}: {}",
                            file.name, e
                        ))
                    })?;
                }
            }
            let mut c = Command::new("/usr/bin/python3");
            c.current_dir(&workspace_dir);
            c.env("PYTHONPATH", input_dir.to_string_lossy().as_ref());
            c
        } else {
            let mut c = Command::new("bwrap");
            c.arg("--unshare-all");
            c.arg("--die-with-parent");
            c.arg("--new-session");

            if config.network_enabled
                || !config.allowed_hosts.is_empty()
                || !config.allowed_ports.is_empty()
            {
                c.arg("--share-net");
            }

            // System read-only mounts
            for sys_path in &["/usr", "/lib", "/lib64", "/bin", "/sbin"] {
                if Path::new(sys_path).exists() {
                    c.arg("--ro-bind").arg(sys_path).arg(sys_path);
                }
            }
            for cert_path in &[
                "/etc/ssl/certs",
                "/etc/ssl/openssl.cnf",
                "/etc/pki",
                "/etc/ca-certificates",
                "/usr/share/ca-certificates",
            ] {
                if Path::new(cert_path).exists() {
                    c.arg("--ro-bind").arg(cert_path).arg(cert_path);
                }
            }

            if Path::new("/etc/resolv.conf").exists()
                && (config.network_enabled
                    || !config.allowed_hosts.is_empty()
                    || !config.allowed_ports.is_empty())
            {
                c.arg("--ro-bind")
                    .arg("/etc/resolv.conf")
                    .arg("/etc/resolv.conf");
            }
            if Path::new("/etc/nsswitch.conf").exists() {
                c.arg("--ro-bind")
                    .arg("/etc/nsswitch.conf")
                    .arg("/etc/nsswitch.conf");
            }
            if Path::new("/etc/hosts").exists() {
                c.arg("--ro-bind").arg("/etc/hosts").arg("/etc/hosts");
            }

            c.arg("--proc").arg("/proc");
            c.arg("--dev").arg("/dev");
            c.arg("--tmpfs").arg("/tmp");

            // Workspace (writable)
            c.arg("--bind").arg(&workspace_dir).arg("/workspace");

            // SDK file (read-only) — /workspace/input/ccos_sdk.py
            c.arg("--ro-bind")
                .arg(&sdk_host_path)
                .arg("/workspace/input/ccos_sdk.py");

            // Additional input files (read-only)
            for file in input_files {
                let sandbox_path = format!("/workspace/input/{}", file.name);
                if file.host_path.exists() {
                    c.arg("--ro-bind").arg(&file.host_path).arg(&sandbox_path);
                }
            }

            // Output directory (writable)
            c.arg("--bind").arg(&output_dir).arg("/workspace/output");

            c.arg("--chdir").arg("/workspace");

            // Resource rlimits
            if let Some(resources) = &config.resources {
                let mem_limit = resources.memory_mb * 1024 * 1024;
                let cpu_limit = resources.cpu_shares;
                let nproc_limit = sandbox_max_nproc();
                unsafe {
                    c.pre_exec(move || {
                        if mem_limit > 0 {
                            let rl = libc::rlimit {
                                rlim_cur: mem_limit as libc::rlim_t,
                                rlim_max: mem_limit as libc::rlim_t,
                            };
                            libc::setrlimit(libc::RLIMIT_RSS, &rl);
                        }
                        if cpu_limit > 0 {
                            let rl = libc::rlimit {
                                rlim_cur: (cpu_limit as u64 * 60) as libc::rlim_t,
                                rlim_max: (cpu_limit as u64 * 60) as libc::rlim_t,
                            };
                            libc::setrlimit(libc::RLIMIT_CPU, &rl);
                        }
                        if let Some(lim) = nproc_limit {
                            let rl = libc::rlimit {
                                rlim_cur: lim as libc::rlim_t,
                                rlim_max: lim as libc::rlim_t,
                            };
                            libc::setrlimit(libc::RLIMIT_NPROC, &rl);
                        }
                        Ok(())
                    });
                }
            }

            c.arg("/usr/bin/python3");
            c.env("PYTHONPATH", "/workspace/input");
            c
        };

        cmd.arg("-c").arg(code);
        cmd.env("PYTHONDONTWRITEBYTECODE", "1");
        cmd.env("PYTHONUNBUFFERED", "1");
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| RuntimeError::Generic(format!("Failed to spawn process: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| RuntimeError::Generic("Failed to capture stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::Generic("Failed to capture stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| RuntimeError::Generic("Failed to capture stderr".to_string()))?;

        // Read stderr asynchronously in a background task
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut r = stderr;
            let _ = r.read_to_end(&mut buf).await;
            buf
        });

        let mut stdin_writer = tokio::io::BufWriter::new(stdin);
        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut visible_stdout = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);

        // Interactive IPC loop
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                let _ = child.kill().await;
                return Err(RuntimeError::Generic(format!(
                    "Execution timeout after {}ms",
                    timeout_ms
                )));
            }

            match tokio::time::timeout(remaining, stdout_reader.next_line()).await {
                Err(_) => {
                    let _ = child.kill().await;
                    return Err(RuntimeError::Generic(format!(
                        "Execution timeout after {}ms",
                        timeout_ms
                    )));
                }
                Ok(Err(e)) => {
                    return Err(RuntimeError::Generic(format!("Stdout read error: {}", e)))
                }
                Ok(Ok(None)) => break, // EOF — process finished writing
                Ok(Ok(Some(line))) => {
                    if let Some(json_str) = line.strip_prefix("CCOS_CALL::") {
                        // Dispatch capability call
                        let result_json = match serde_json::from_str::<serde_json::Value>(json_str)
                        {
                            Err(e) => {
                                log::warn!("[sandbox] Invalid CCOS_CALL JSON: {}", e);
                                serde_json::json!({"success": false, "error": format!("Invalid call JSON: {}", e)})
                            }
                            Ok(call) => {
                                let cap_id = call["cap"].as_str().unwrap_or("").to_string();
                                let inputs = call["inputs"].clone();
                                match dispatcher(cap_id.clone(), inputs).await {
                                    Ok(v) => {
                                        log::debug!("[sandbox] CCOS_CALL {} → ok", cap_id);
                                        serde_json::json!({"success": true, "value": v})
                                    }
                                    Err(e) => {
                                        log::warn!("[sandbox] CCOS_CALL {} → error: {}", cap_id, e);
                                        serde_json::json!({"success": false, "error": e.to_string()})
                                    }
                                }
                            }
                        };
                        // Write result to Python's stdin
                        let response = format!(
                            "CCOS_RESULT::{}\n",
                            serde_json::to_string(&result_json).unwrap_or_else(|_| {
                                r#"{"success":false,"error":"serialise"}"#.to_string()
                            })
                        );
                        if let Err(e) = stdin_writer.write_all(response.as_bytes()).await {
                            log::warn!("[sandbox] Failed to write CCOS_RESULT to stdin: {}", e);
                        }
                        let _ = stdin_writer.flush().await;
                    } else {
                        visible_stdout.push_str(&line);
                        visible_stdout.push('\n');
                    }
                }
            }
        }

        // Close stdin — signals EOF to Python if it's still reading
        drop(stdin_writer);

        // Wait for process to exit
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let exit_status = match tokio::time::timeout(remaining, child.wait()).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Err(RuntimeError::Generic(format!("Process error: {}", e))),
            Err(_) => {
                let _ = child.kill().await;
                return Err(RuntimeError::Generic(format!(
                    "Execution timeout after {}ms (wait phase)",
                    timeout_ms
                )));
            }
        };

        let stderr_buf = stderr_task.await.unwrap_or_default();
        let mut stderr_str = String::from_utf8_lossy(&stderr_buf).to_string();

        if !exit_status.success()
            && (stderr_str.contains("Creating new namespace failed")
                || stderr_str.contains("Resource temporarily unavailable"))
        {
            stderr_str.push_str(
                "\n\n[CCOS] bwrap failed to create user namespace. \
                To fix: run 'sudo sysctl -w user.max_user_namespaces=15000'. \
                Or set CCOS_EXECUTE_NO_SANDBOX=1 (less secure).",
            );
        }

        // Collect output files
        let mut output_files = HashMap::new();
        if let Ok(mut entries) = fs::read_dir(&output_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if is_allowed_extension(&ext_str) {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            if let Ok(content) = fs::read(&path).await {
                                if content.len() <= 10 * 1024 * 1024 {
                                    output_files.insert(name, content);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(SandboxExecutionResult {
            success: exit_status.success(),
            stdout: visible_stdout,
            stderr: stderr_str,
            exit_code: exit_status.code(),
            output_files,
        })
    }

    /// Generic execution method for different runtimes
    async fn execute_with_runtime(
        &self,
        runtime: &str,
        code: &str,
        input_files: &[InputFile],
        config: &SandboxConfig,
        dependencies: Option<&[String]>,
        dep_manager: Option<&super::DependencyManager>,
    ) -> Result<SandboxExecutionResult, RuntimeError> {
        use tracing::{info, warn};
        // Security scan
        self.scanner
            .scan(code)
            .map_err(|e| RuntimeError::Generic(format!("Security violation: {}", e)))?;

        // Create temp directories
        let work_dir = tempfile::tempdir()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create temp dir: {}", e)))?;
        let input_dir = work_dir.path().join("input");
        let output_dir = work_dir.path().join("output");

        fs::create_dir_all(&input_dir)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to create input dir: {}", e)))?;
        fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to create output dir: {}", e)))?;

        if let (Some(deps), Some(manager)) = (dependencies, dep_manager) {
            info!("Checking dependencies: {:?}", deps);
            let resolutions = manager.resolve_dependencies(deps);

            let mut packages_to_install = Vec::new();
            for resolution in resolutions {
                match resolution {
                    super::DependencyResolution::AlreadyAvailable => {}
                    super::DependencyResolution::AutoInstall { package } => {
                        info!("Auto-installing package: {}", package);
                        packages_to_install.push(package);
                    }
                    super::DependencyResolution::RequiresApproval { package } => {
                        warn!("Package {} requires approval, skipping", package);
                        return Err(RuntimeError::Generic(
                            format!("Package '{}' requires approval. Add to auto_approved list or approve manually.", package)
                        ));
                    }
                    super::DependencyResolution::Blocked { package, reason } => {
                        warn!("Package {} is blocked: {}", package, reason);
                        return Err(RuntimeError::Generic(format!(
                            "Package '{}' is blocked: {}",
                            package, reason
                        )));
                    }
                }
            }

            // Install packages before running sandbox
            if !packages_to_install.is_empty() {
                info!("Installing {} packages: {:?}", runtime, packages_to_install);
                match runtime {
                    "python" => {
                        manager
                            .install_packages(&packages_to_install, &work_dir.path().to_path_buf())
                            .await
                            .map_err(|e| {
                                RuntimeError::Generic(format!(
                                    "Failed to install python packages: {}",
                                    e
                                ))
                            })?;
                    }
                    "javascript" => {
                        manager
                            .install_npm_packages(
                                &packages_to_install,
                                &work_dir.path().to_path_buf(),
                            )
                            .await
                            .map_err(|e| {
                                RuntimeError::Generic(format!(
                                    "Failed to install npm packages: {}",
                                    e
                                ))
                            })?;
                    }
                    _ => {
                        warn!(
                            "Unknown runtime for package installation: {}, skipping",
                            runtime
                        );
                    }
                }
            }
        }

        if no_sandbox_requested() {
            log::warn!(
                "CCOS_EXECUTE_NO_SANDBOX is set; running {} without bubblewrap (less secure)",
                runtime
            );
            return Self::execute_unjailed(
                runtime,
                code,
                input_files,
                &work_dir,
                &input_dir,
                &output_dir,
                config,
            )
            .await;
        }

        // Build bwrap command
        let mut cmd = Command::new("bwrap");

        // Basic isolation flags
        cmd.arg("--unshare-all");
        cmd.arg("--die-with-parent");
        cmd.arg("--new-session");

        // Conditionally enable network if allowlists are present or explicitly enabled
        if config.network_enabled
            || !config.allowed_hosts.is_empty()
            || !config.allowed_ports.is_empty()
        {
            cmd.arg("--share-net");
        }

        // Mount read-only system directories
        cmd.arg("--ro-bind").arg("/usr").arg("/usr");
        cmd.arg("--ro-bind").arg("/lib").arg("/lib");
        cmd.arg("--ro-bind").arg("/lib64").arg("/lib64");
        if Path::new("/bin").exists() {
            cmd.arg("--ro-bind").arg("/bin").arg("/bin");
        }
        if Path::new("/sbin").exists() {
            cmd.arg("--ro-bind").arg("/sbin").arg("/sbin");
        }
        if Path::new("/etc/ssl/certs").exists() {
            cmd.arg("--ro-bind")
                .arg("/etc/ssl/certs")
                .arg("/etc/ssl/certs");
        }
        if Path::new("/etc/ssl/openssl.cnf").exists() {
            cmd.arg("--ro-bind")
                .arg("/etc/ssl/openssl.cnf")
                .arg("/etc/ssl/openssl.cnf");
        }
        if Path::new("/etc/pki").exists() {
            // Some distros use /etc/pki for public certs too
            cmd.arg("--ro-bind").arg("/etc/pki").arg("/etc/pki");
        }
        if Path::new("/etc/ca-certificates").exists() {
            cmd.arg("--ro-bind")
                .arg("/etc/ca-certificates")
                .arg("/etc/ca-certificates");
        }
        if Path::new("/usr/share/ca-certificates").exists() {
            cmd.arg("--ro-bind")
                .arg("/usr/share/ca-certificates")
                .arg("/usr/share/ca-certificates");
        }
        if Path::new("/etc/resolv.conf").exists()
            && (config.network_enabled
                || !config.allowed_hosts.is_empty()
                || !config.allowed_ports.is_empty())
        {
            cmd.arg("--ro-bind")
                .arg("/etc/resolv.conf")
                .arg("/etc/resolv.conf");
        }
        if Path::new("/etc/nsswitch.conf").exists() {
            cmd.arg("--ro-bind")
                .arg("/etc/nsswitch.conf")
                .arg("/etc/nsswitch.conf");
        }
        if Path::new("/etc/hosts").exists() {
            cmd.arg("--ro-bind").arg("/etc/hosts").arg("/etc/hosts");
        }

        // Mount proc and dev (minimal)
        cmd.arg("--proc").arg("/proc");
        cmd.arg("--dev").arg("/dev");

        // Mount tmpfs for /tmp
        cmd.arg("--tmpfs").arg("/tmp");

        // Create and mount workspace
        let workspace_dir = work_dir.path().join("workspace");
        fs::create_dir_all(&workspace_dir)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to create workspace dir: {}", e)))?;
        cmd.arg("--bind").arg(&workspace_dir).arg("/workspace");

        // Mount input files (read-only)
        for file in input_files {
            let sandbox_path = format!("/workspace/input/{}", file.name);
            if file.host_path.exists() {
                cmd.arg("--ro-bind").arg(&file.host_path).arg(&sandbox_path);
            }
        }

        // Mount output directory (writable)
        cmd.arg("--bind").arg(&output_dir).arg("/workspace/output");

        // Set working directory
        cmd.arg("--chdir").arg("/workspace");

        // Resource limits (if specified)
        if let Some(resources) = &config.resources {
            if resources.memory_mb > 0 {
                cmd.arg("--setenv")
                    .arg("MEMORY_LIMIT_MB")
                    .arg(resources.memory_mb.to_string());
            }

            let mem_limit = resources.memory_mb * 1024 * 1024;
            let cpu_limit = resources.cpu_shares; // Simplified mapping to CPU seconds for this demo if needed
            let nproc_limit = sandbox_max_nproc();

            unsafe {
                cmd.pre_exec(move || {
                    if mem_limit > 0 {
                        let rlimit = libc::rlimit {
                            rlim_cur: mem_limit as libc::rlim_t,
                            rlim_max: mem_limit as libc::rlim_t,
                        };
                        // RLIMIT_RSS is preferred for Node.js over RLIMIT_AS
                        if libc::setrlimit(libc::RLIMIT_RSS, &rlimit) != 0 {}
                    }

                    if cpu_limit > 0 {
                        let rlimit = libc::rlimit {
                            rlim_cur: (cpu_limit as u64 * 60) as libc::rlim_t, // Increase to 60s per share
                            rlim_max: (cpu_limit as u64 * 60) as libc::rlim_t,
                        };
                        if libc::setrlimit(libc::RLIMIT_CPU, &rlimit) != 0 {}
                    }

                    if let Some(limit) = nproc_limit {
                        let nproc_rlimit = libc::rlimit {
                            rlim_cur: limit as libc::rlim_t,
                            rlim_max: limit as libc::rlim_t,
                        };
                        libc::setrlimit(libc::RLIMIT_NPROC, &nproc_rlimit);
                    }

                    Ok(())
                });
            }
        }

        // Runtime specific setup
        match runtime {
            "python" => {
                cmd.arg("/usr/bin/python3");
                cmd.arg("-c");
                cmd.arg(code);
                cmd.env("PYTHONDONTWRITEBYTECODE", "1");
                cmd.env("PYTHONUNBUFFERED", "1");
            }
            "javascript" => {
                cmd.arg("/usr/bin/node");
                cmd.arg("-e");
                cmd.arg(code);
            }
            _ => {
                return Err(RuntimeError::Generic(format!(
                    "Unsupported runtime: {}",
                    runtime
                )))
            }
        }

        // Set resource limits via ulimit (soft)
        cmd.env("PYTHONDONTWRITEBYTECODE", "1");
        cmd.env("PYTHONUNBUFFERED", "1");

        // Capture output
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let timeout_ms = config
            .resources
            .as_ref()
            .map(|r| {
                if r.timeout_ms > 0 {
                    r.timeout_ms
                } else {
                    30000
                }
            })
            .unwrap_or(30000);

        // Spawn and wait
        let mut child = cmd
            .spawn()
            .map_err(|e| RuntimeError::Generic(format!("Failed to spawn bwrap: {}", e)))?;

        // Take stdout and stderr handles before waiting
        let mut stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::Generic("Failed to capture stdout".to_string()))?;
        let mut stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| RuntimeError::Generic("Failed to capture stderr".to_string()))?;

        // Use tokio::time::timeout with wait
        let timeout_duration = Duration::from_millis(timeout_ms as u64);
        let wait_result = timeout(timeout_duration, child.wait()).await;

        let exit_status = match wait_result {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => {
                return Err(RuntimeError::Generic(format!("Process error: {}", e)));
            }
            Err(_) => {
                // Timeout - kill process
                let _ = child.kill().await;
                return Err(RuntimeError::Generic(format!(
                    "Execution timeout after {}ms",
                    timeout_ms
                )));
            }
        };

        // Read stdout and stderr after process completes
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();

        use tokio::io::AsyncReadExt;
        stdout_handle
            .read_to_end(&mut stdout_buf)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read stdout: {}", e)))?;
        stderr_handle
            .read_to_end(&mut stderr_buf)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read stderr: {}", e)))?;

        let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
        let mut stderr = String::from_utf8_lossy(&stderr_buf).to_string();

        // When bwrap fails due to user namespace limits, add a clear fix hint
        if !exit_status.success()
            && (stderr.contains("Creating new namespace failed")
                || stderr.contains("Resource temporarily unavailable"))
        {
            const BWRAP_NAMESPACE_HINT: &str = "\n\n[CCOS] bwrap failed to create user namespace. \
                To fix: run 'sudo sysctl -w user.max_user_namespaces=15000' (or add to /etc/sysctl.d). \
                In constrained environments (e.g. some containers) you can set CCOS_EXECUTE_NO_SANDBOX=1 \
                to run code without the sandbox (less secure).";
            stderr.push_str(BWRAP_NAMESPACE_HINT);
        }

        // Collect output files
        let mut output_files = HashMap::new();
        if let Ok(mut entries) = fs::read_dir(&output_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    // Check extension
                    if let Some(ext) = path.extension() {
                        let ext = ext.to_string_lossy().to_lowercase();
                        if is_allowed_extension(&ext) {
                            // Read and base64 encode
                            match fs::read(&path).await {
                                Ok(content) => {
                                    if content.len() <= 10 * 1024 * 1024 {
                                        // 10MB limit
                                        output_files.insert(name, content);
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to read output file {:?}: {}", path, e);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(SandboxExecutionResult {
            success: exit_status.success(),
            stdout,
            stderr,
            exit_code: exit_status.code(),
            output_files,
        })
    }

    /// Run code without bubblewrap (when CCOS_EXECUTE_NO_SANDBOX=1).
    /// Uses the same workspace layout: work_dir with input/ and output/ subdirs.
    async fn execute_unjailed(
        runtime: &str,
        code: &str,
        input_files: &[InputFile],
        work_dir: &tempfile::TempDir,
        input_dir: &Path,
        output_dir: &Path,
        config: &SandboxConfig,
    ) -> Result<SandboxExecutionResult, RuntimeError> {
        use tokio::io::AsyncReadExt;

        for file in input_files {
            if file.host_path.exists() {
                let dest = input_dir.join(&file.name);
                fs::copy(&file.host_path, &dest).await.map_err(|e| {
                    RuntimeError::Generic(format!("Failed to copy input file {}: {}", file.name, e))
                })?;
            }
        }

        let timeout_ms = config
            .resources
            .as_ref()
            .map(|r| {
                if r.timeout_ms > 0 {
                    r.timeout_ms
                } else {
                    30000
                }
            })
            .unwrap_or(30000);

        let mut cmd = Command::new(match runtime {
            "python" => "/usr/bin/python3",
            "javascript" => "/usr/bin/node",
            _ => {
                return Err(RuntimeError::Generic(format!(
                    "Unsupported runtime: {}",
                    runtime
                )));
            }
        });

        cmd.current_dir(work_dir.path());
        cmd.env("PYTHONDONTWRITEBYTECODE", "1");
        cmd.env("PYTHONUNBUFFERED", "1");

        match runtime {
            "python" => {
                cmd.arg("-c").arg(code);
            }
            "javascript" => {
                cmd.arg("-e").arg(code);
            }
            _ => {}
        }

        if let Some(resources) = &config.resources {
            let mem_limit = resources.memory_mb * 1024 * 1024;
            let cpu_limit = resources.cpu_shares;
            let nproc_limit = sandbox_max_nproc();
            unsafe {
                cmd.pre_exec(move || {
                    if mem_limit > 0 {
                        let rlimit = libc::rlimit {
                            rlim_cur: mem_limit as libc::rlim_t,
                            rlim_max: mem_limit as libc::rlim_t,
                        };
                        if libc::setrlimit(libc::RLIMIT_RSS, &rlimit) != 0 {}
                    }
                    if cpu_limit > 0 {
                        let rlimit = libc::rlimit {
                            rlim_cur: (cpu_limit as u64 * 60) as libc::rlim_t,
                            rlim_max: (cpu_limit as u64 * 60) as libc::rlim_t,
                        };
                        if libc::setrlimit(libc::RLIMIT_CPU, &rlimit) != 0 {}
                    }
                    if let Some(limit) = nproc_limit {
                        let nproc_rlimit = libc::rlimit {
                            rlim_cur: limit as libc::rlim_t,
                            rlim_max: limit as libc::rlim_t,
                        };
                        libc::setrlimit(libc::RLIMIT_NPROC, &nproc_rlimit);
                    }
                    Ok(())
                });
            }
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| RuntimeError::Generic(format!("Failed to spawn {}: {}", runtime, e)))?;

        let mut stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::Generic("Failed to capture stdout".to_string()))?;
        let mut stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| RuntimeError::Generic("Failed to capture stderr".to_string()))?;

        let timeout_duration = Duration::from_millis(timeout_ms as u64);
        let wait_result = timeout(timeout_duration, child.wait()).await;

        let exit_status = match wait_result {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => {
                return Err(RuntimeError::Generic(format!("Process error: {}", e)));
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(RuntimeError::Generic(format!(
                    "Execution timeout after {}ms",
                    timeout_ms
                )));
            }
        };

        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        stdout_handle
            .read_to_end(&mut stdout_buf)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read stdout: {}", e)))?;
        stderr_handle
            .read_to_end(&mut stderr_buf)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read stderr: {}", e)))?;

        let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
        let stderr = String::from_utf8_lossy(&stderr_buf).to_string();

        let mut output_files = HashMap::new();
        if let Ok(mut entries) = fs::read_dir(output_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    if let Some(ext) = path.extension() {
                        let ext = ext.to_string_lossy().to_lowercase();
                        if is_allowed_extension(&ext) {
                            if let Ok(content) = fs::read(&path).await {
                                if content.len() <= 10 * 1024 * 1024 {
                                    output_files.insert(name, content);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(SandboxExecutionResult {
            success: exit_status.success(),
            stdout,
            stderr,
            exit_code: exit_status.code(),
            output_files,
        })
    }
}

/// Check if file extension is allowed
fn is_allowed_extension(ext: &str) -> bool {
    let allowed = [
        "png", "jpg", "jpeg", "svg", "gif", "csv", "json", "txt", "md", "html", "pdf", "xlsx",
        "parquet",
    ];
    allowed.contains(&ext)
}

/// Security scanner for code
pub struct SecurityScanner {
    patterns: Vec<regex::Regex>,
}

impl SecurityScanner {
    /// Create scanner with blocked patterns
    pub fn new(patterns: &[String]) -> Result<Self, String> {
        let mut compiled = Vec::new();
        for pattern in patterns {
            match regex::Regex::new(pattern) {
                Ok(regex) => compiled.push(regex),
                Err(e) => return Err(format!("Invalid pattern '{}': {}", pattern, e)),
            }
        }
        Ok(Self { patterns: compiled })
    }

    /// Scan code for security violations
    pub fn scan(&self, code: &str) -> Result<(), String> {
        for pattern in &self.patterns {
            if pattern.is_match(code) {
                return Err(format!("Blocked pattern detected: {}", pattern.as_str()));
            }
        }
        Ok(())
    }
}
