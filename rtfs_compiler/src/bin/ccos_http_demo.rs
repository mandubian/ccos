use rtfs_compiler::ccos::environment::{CCOSEnvironment, CCOSBuilder};
use rtfs_compiler::runtime::execution_outcome::ExecutionOutcome;

fn main() {
    // Build a CCOS environment with HTTP mocking disabled and allowlist for httpbin
    let builder = CCOSBuilder::new()
        .http_mocking(false)
        .http_allow_hosts(vec!["httpbin.org".to_string()])
        .verbose(true);

    let env = builder.build().expect("Failed to build environment");

    // Optional: print whether OPENWEATHERMAP_ORG_API_KEY is present in the process environment
    match std::env::var("OPENWEATHERMAP_ORG_API_KEY") {
        Ok(k) => println!("OPENWEATHERMAP_ORG_API_KEY is set (length {})", k.len()),
        Err(_) => println!("OPENWEATHERMAP_ORG_API_KEY is NOT set in this process environment"),
    }

    // Call the low-level host capability `ccos.network.http-fetch` directly against httpbin.org
    let expr = r#"(call "ccos.network.http-fetch" {:url "https://httpbin.org/get"})"#;
    println!("Executing expression: {}", expr);

    match env.execute_code(expr) {
        Ok(outcome) => match outcome {
            ExecutionOutcome::Complete(v) => {
                println!("Result: {:?}", v);
            }
            ExecutionOutcome::RequiresHost(h) => {
                println!("Host call required: {:?}", h);
            }
        },
        Err(e) => println!("Execution error: {:?}", e),
    }
}
