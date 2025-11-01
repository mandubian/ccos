use rtfs_compiler::ccos::environment::{CCOSBuilder, CCOSEnvironment};
use rtfs_compiler::runtime::execution_outcome::ExecutionOutcome;
use std::fs;

fn main() {
    // Build a CCOS environment with HTTP mocking disabled and allowlist for openweathermap.org
    let builder = CCOSBuilder::new()
        .http_mocking(false)
        .http_allow_hosts(vec!["api.openweathermap.org".to_string()])
        .verbose(true);

    let env = builder.build().expect("Failed to build environment");

    // Check for API key
    match std::env::var("OPENWEATHERMAP_ORG_API_KEY") {
        Ok(k) => println!("OPENWEATHERMAP_ORG_API_KEY is set (length {})", k.len()),
        Err(_) => println!("OPENWEATHERMAP_ORG_API_KEY is NOT set in this process environment"),
    }

    // Load the capability file
    let capability_path = "../capabilities/openweather api/capability.rtfs";
    let capability_code = match fs::read_to_string(capability_path) {
        Ok(code) => {
            println!("Loaded capability from: {}", capability_path);
            code
        }
        Err(e) => {
            println!("Failed to load capability file: {}", e);
            return;
        }
    };

    // First, load the capability definition
    println!("Loading capability definition...");
    match env.execute_code(&capability_code) {
        Ok(outcome) => match outcome {
            ExecutionOutcome::Complete(v) => {
                println!("Capability loaded successfully: {:?}", v);
            }
            ExecutionOutcome::RequiresHost(h) => {
                println!("Host call required during capability loading: {:?}", h);
            }
        },
        Err(e) => {
            println!("Failed to load capability: {:?}", e);
            return;
        }
    }

    // Now test the capability with a simple weather request
    println!("\nTesting OpenWeather API capability...");
    let test_expr = r#"
    (call "openweather api" "/data/2.5/weather?q=london")
    "#;

    println!("Executing test: {}", test_expr);

    match env.execute_code(test_expr) {
        Ok(outcome) => match outcome {
            ExecutionOutcome::Complete(v) => {
                println!("Weather API Result: {:?}", v);
            }
            ExecutionOutcome::RequiresHost(h) => {
                println!("Host call required: {:?}", h);
            }
        },
        Err(e) => println!("Execution error: {:?}", e),
    }
}
