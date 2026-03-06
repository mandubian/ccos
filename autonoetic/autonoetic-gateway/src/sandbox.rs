//! Sandbox runner supporting bubblewrap, docker, and firecracker.

use std::process::{Child, Command, Stdio};

const DOCKER_IMAGE_ENV: &str = "AUTONOETIC_DOCKER_IMAGE";
const FIRECRACKER_CONFIG_ENV: &str = "AUTONOETIC_FIRECRACKER_CONFIG";

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

pub struct SandboxRunner {
    pub process: Child,
    pub driver: SandboxDriverKind,
}

impl SandboxRunner {
    /// Spawn with the default bubblewrap driver.
    pub fn spawn(agent_dir: &str, entrypoint: &str) -> anyhow::Result<Self> {
        Self::spawn_with_driver(SandboxDriverKind::Bubblewrap, agent_dir, entrypoint)
    }

    /// Spawn using the manifest-declared driver name.
    pub fn spawn_for_driver(driver_name: &str, agent_dir: &str, entrypoint: &str) -> anyhow::Result<Self> {
        let driver = SandboxDriverKind::parse(driver_name)?;
        Self::spawn_with_driver(driver, agent_dir, entrypoint)
    }

    pub fn spawn_with_driver(
        driver: SandboxDriverKind,
        agent_dir: &str,
        entrypoint: &str,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(!entrypoint.trim().is_empty(), "entrypoint must not be empty");
        let (program, args) = match driver {
            SandboxDriverKind::Bubblewrap => bubblewrap_command(agent_dir, entrypoint)?,
            SandboxDriverKind::Docker => docker_command(agent_dir, entrypoint)?,
            SandboxDriverKind::MicroVm => microvm_command(entrypoint)?,
        };

        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        Ok(Self { process: child, driver })
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
        "/workspace".to_string(),
        "--unshare-all".to_string(),
        "--".to_string(),
        program,
    ];
    argv.extend(args);
    Ok(("bwrap".to_string(), argv))
}

fn docker_command(agent_dir: &str, entrypoint: &str) -> anyhow::Result<(String, Vec<String>)> {
    let image = std::env::var(DOCKER_IMAGE_ENV)
        .map_err(|_| anyhow::anyhow!("Missing required environment variable {}", DOCKER_IMAGE_ENV))?;
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
    let cfg = std::env::var(FIRECRACKER_CONFIG_ENV)
        .map_err(|_| anyhow::anyhow!("Missing required environment variable {}", FIRECRACKER_CONFIG_ENV))?;
    let argv = vec![
        "--config-file".to_string(),
        cfg,
    ];
    Ok(("firecracker".to_string(), argv))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(argv[7], "--");
        assert_eq!(argv[8], "python");
        assert_eq!(argv[9], "main.py");
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
        let err = microvm_command("ignored")
            .expect_err("microvm command should fail without env");
        assert!(
            err.to_string().contains(FIRECRACKER_CONFIG_ENV),
            "error should mention missing firecracker env"
        );
        if let Some(v) = old {
            std::env::set_var(FIRECRACKER_CONFIG_ENV, v);
        }
    }
}
