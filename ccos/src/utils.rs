pub mod value_conversion;

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

pub async fn run_example_with_args(
    example_path: &PathBuf,

    args: Vec<&str>,
) -> Result<(String, String), Box<dyn Error>> {
    let cargo_bin_name = example_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("Invalid example path: {}", example_path.display()))?;

    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();

    let mut cmd = Command::new("cargo");

    cmd.current_dir(&workspace_root);

    cmd.args(["run", "--example", cargo_bin_name, "--"]);

    cmd.args(args);

    eprintln!(
        "DEBUG: Running command: {:?} in directory: {:?}",
        cmd, &workspace_root
    );

    let output = cmd.output()?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!(
            "Example command failed: {:?}\nStdout: {}\nStderr: {}",
            cmd, stdout, stderr
        )
        .into());
    }

    Ok((stdout, stderr))
}
