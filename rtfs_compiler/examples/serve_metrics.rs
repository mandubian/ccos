// Example: minimal, test-only Prometheus metrics server
// Build/run with feature enabled:
//   cargo run --example serve_metrics --features metrics_exporter --quiet

#[cfg(not(feature = "metrics_exporter"))]
fn main() {
    eprintln!(
        "metrics_exporter feature not enabled. Run with: cargo run --example serve_metrics --features metrics_exporter"
    );
}

#[cfg(feature = "metrics_exporter")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rtfs_compiler::ccos::causal_chain::CausalChain;
    use rtfs_compiler::runtime::metrics_exporter::start_metrics_server;
    use std::sync::{Arc, Mutex};

    // Create an empty causal chain. Even without events, exporter renders zeroed gauges.
    let chain = Arc::new(Mutex::new(CausalChain::new()?));

    // Bind address from CLI arg or default to localhost:9898
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9898".to_string());

    println!(
        "Serving Prometheus metrics at http://{}/metrics (one request will stop the server)",
        addr
    );

    // Start the tiny single-request server and block until the request is served.
    let handle = start_metrics_server(chain.clone(), &addr)?;
    let _ = handle.join();
    Ok(())
}
