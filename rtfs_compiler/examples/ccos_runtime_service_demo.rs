// Console demo for the CCOS runtime service: shows how to embed CCOS cleanly
// Build: cargo run --example ccos_runtime_service_demo --manifest-path rtfs_compiler/Cargo.toml

use std::sync::Arc;
use clap::Parser;
use rtfs_compiler::ccos::{CCOS, runtime_service};

#[derive(Debug, Parser)]
struct Args {
    /// Natural language goal to process
    #[arg(short, long, default_value = "Summarize this project and list key modules")]
    goal: String,
    /// Verbose event printing
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async move {
    let args = Args::parse();

    // Initialize CCOS and runtime service
    let ccos = Arc::new(CCOS::new().await.expect("init CCOS"));
    let handle = runtime_service::start_service(Arc::clone(&ccos)).await;

    // Subscribe to events
    let mut rx = handle.subscribe();

    // Send start command
    let ctx = runtime_service::default_controlled_context();
    let _ = handle.commands().send(runtime_service::RuntimeCommand::Start { goal: args.goal.clone(), context: ctx }).await;

    // Drain a few events and print compactly
    use tokio::time::{timeout, Duration};
    loop {
        match timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Ok(evt)) => {
                match evt {
                    runtime_service::RuntimeEvent::Started { intent_id, goal } => {
                        println!("Started: intent={} goal={}", intent_id, goal);
                    }
                    runtime_service::RuntimeEvent::Status { intent_id, status } => {
                        if args.verbose { println!("Status: intent={} status={}", intent_id, status); }
                    }
                    runtime_service::RuntimeEvent::Step { intent_id, desc } => {
                        if args.verbose { println!("Step: intent={} desc={}", intent_id, desc); }
                    }
                    runtime_service::RuntimeEvent::Result { intent_id, result } => {
                        println!("Result: intent={} success={} value={:?}", intent_id, result.success, result.value);
                        break;
                    }
                    runtime_service::RuntimeEvent::Error { message } => {
                        eprintln!("Error: {}", message);
                        break;
                    }
                    runtime_service::RuntimeEvent::Heartbeat => {
                        if args.verbose { println!("heartbeat"); }
                    }
                    runtime_service::RuntimeEvent::Stopped => {
                        println!("Stopped");
                        break;
                    }
                }
            }
            Ok(Err(_)) => { println!("event channel closed"); break; }
            Err(_) => { println!("timeout waiting for result"); break; }
        }
    }
    });
}
