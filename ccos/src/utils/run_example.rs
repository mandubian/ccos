use std::path::Path;
use tokio::process::Command;

/// Run an example binary with arguments and capture stdout/stderr.
/// Used by tests like `test_autonomous_planner` to exercise examples end-to-end.
pub async fn run_example_with_args(
    example_path: &Path,
    args: Vec<&str>,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    // Infer the binary name from the file stem (e.g., examples/foo.rs -> foo)
    let bin_name = example_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Invalid example path")?;

    // Run from the crate root to ensure Cargo sees the binary target.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut cmd = Command::new("cargo");
    cmd.current_dir(manifest_dir)
        .arg("run")
        .arg("--quiet")
        .arg("--bin")
        .arg(bin_name)
        .arg("--");

    for a in args {
        cmd.arg(a);
    }

    let output = cmd.output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok((stdout, stderr))
    } else {
        Err(format!(
            "Example '{}' failed: status {:?}\nstdout:\n{}\nstderr:\n{}",
            bin_name, output.status.code(), stdout, stderr
        )
        .into())
    }
}
