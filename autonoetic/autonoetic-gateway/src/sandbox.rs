//! Bubblewrap sandbox runner.

use std::process::{Child, Command, Stdio};

pub struct SandboxRunner {
    pub process: Child,
}

impl SandboxRunner {
    /// Spawn an agent inside a Bubblewrap sandbox.
    ///
    /// `agent_dir` is bind-mounted read-write at `/workspace`.
    /// The `entrypoint` string is split on whitespace so multi-word
    /// commands like `"python main.py"` work correctly.
    pub fn spawn(agent_dir: &str, entrypoint: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = entrypoint.split_whitespace().collect();
        anyhow::ensure!(!parts.is_empty(), "entrypoint must not be empty");

        let program = parts[0];
        let args = &parts[1..];

        let child = Command::new("bwrap")
            .arg("--ro-bind")
            .arg("/")
            .arg("/")
            .arg("--bind")
            .arg(agent_dir)
            .arg("/workspace")
            .arg("--unshare-all")
            .arg("--")
            .arg(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(Self { process: child })
    }
}
