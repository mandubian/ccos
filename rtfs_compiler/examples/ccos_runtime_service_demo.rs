// Console demo for the CCOS runtime service: shows how to embed CCOS cleanly
// Build: cargo run --example ccos_runtime_service_demo --manifest-path rtfs_compiler/Cargo.toml

use clap::Parser;
use rtfs_compiler::ccos::{runtime_service, CCOS};
use std::sync::Arc;

#[derive(Debug, Parser)]
struct Args {
    /// Natural language goal to process
    #[arg(
        short,
        long,
        default_value = "Summarize this project and list key modules"
    )]
    goal: String,
    /// Verbose event printing
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
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
        let _ = handle
            .commands()
            .send(runtime_service::RuntimeCommand::Start {
                goal: args.goal.clone(),
                context: ctx,
            })
            .await;

        // Drain a few events and print compactly
        use tokio::time::{timeout, Duration};
        loop {
            match timeout(Duration::from_secs(15), rx.recv()).await {
                Ok(Ok(evt)) => match evt {
                    runtime_service::RuntimeEvent::Started { intent_id, goal } => {
                        println!("Started: intent={} goal={}", intent_id, goal);
                    }
                    runtime_service::RuntimeEvent::Status { intent_id, status } => {
                        if args.verbose {
                            println!("Status: intent={} status={}", intent_id, status);
                        }
                    }
                    runtime_service::RuntimeEvent::Step { intent_id, desc } => {
                        if args.verbose {
                            println!("Step: intent={} desc={}", intent_id, desc);
                        }
                    }
                    runtime_service::RuntimeEvent::Result { intent_id, result } => {
                        println!("Result: intent={} result={}", intent_id, result);
                        break;
                    }
                    runtime_service::RuntimeEvent::Error { message } => {
                        eprintln!("Error: {}", message);
                        break;
                    }
                    runtime_service::RuntimeEvent::Heartbeat => {
                        if args.verbose {
                            println!("heartbeat");
                        }
                    }
                    runtime_service::RuntimeEvent::Stopped => {
                        println!("Stopped");
                        break;
                    }
                    runtime_service::RuntimeEvent::GraphGenerated {
                        root_id,
                        nodes: _,
                        edges: _,
                    } => {
                        if args.verbose {
                            println!("GraphGenerated: root_id={}", root_id);
                        }
                    }
                    runtime_service::RuntimeEvent::PlanGenerated {
                        intent_id,
                        plan_id,
                        rtfs_code,
                    } => {
                        if args.verbose {
                            println!("PlanGenerated: intent={} plan={}", intent_id, plan_id);
                        }
                    }
                    runtime_service::RuntimeEvent::StepLog {
                        step,
                        status,
                        message,
                        details,
                    } => {
                        if args.verbose {
                            println!("StepLog: {} [{}] {} {:?}", step, status, message, details);
                        }
                    }
                    runtime_service::RuntimeEvent::ReadyForNext { next_step } => {
                        if args.verbose {
                            println!("ReadyForNext: {}", next_step);
                        }
                    }
                },
                Ok(Err(_)) => {
                    println!("event channel closed");
                    break;
                }
                Err(_) => {
                    println!("timeout waiting for result");
                    break;
                }
            }
        }
    });
}
