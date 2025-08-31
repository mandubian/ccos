// Demonstrate rejection when a plan references an unknown capability
// Run: cargo run --example unknown_capability_demo --manifest-path rtfs_compiler/Cargo.toml

use rtfs_compiler::ccos::{CCOS, types::Plan};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Build CCOS and a synthetic plan that calls a non-existent capability
    let ccos = CCOS::new().await.expect("init CCOS");
    let body = r#"(do (step "Try Unknown" (call :ccos.does.not.exist "ping")))"#;
    let plan = Plan::new_rtfs(body.to_string(), vec![]);

    match ccos.preflight_validate_capabilities(&plan).await {
        Ok(_) => {
            eprintln!("Unexpected: preflight passed (capability should be unknown)");
        }
        Err(e) => {
            println!("Preflight error (as expected): {}", e);
        }
    }
}
