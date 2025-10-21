use rtfs_compiler::ccos::environment::CCOSBuilder;
use rtfs_compiler::runtime::execution_outcome::ExecutionOutcome;

fn main() {
    // Build a CCOS environment with HTTP mocking disabled and allowlist for openweathermap
    let builder = CCOSBuilder::new()
        .http_mocking(false)
        .http_allow_hosts(vec![
            "openweathermap.org".to_string(),
            "api.openweathermap.org".to_string(),
        ])
        .verbose(true);

    let env = builder.build().expect("Failed to build environment");

    // Attempt to load a discovered RTFS capability manifest from disk and execute it
    // which will register the capability into the environment's marketplace.
    // The resolver previously saved discovery to: ./capabilities/openweather api/capability.rtfs
    let manifest_path = std::path::Path::new("../capabilities/openweather api/capability.rtfs");
    if manifest_path.exists() {
        println!("Found manifest at {:?}, executing it to register capability", manifest_path);
        match env.execute_file(manifest_path.to_str().unwrap()) {
            Ok(out) => println!("Loaded manifest: {:?}", out),
            Err(e) => eprintln!("Failed to load manifest into environment: {:?}", e),
        }
    } else {
        println!("No discovered manifest found at {:?}", manifest_path);
    }

    // Optional: print whether OPENWEATHERMAP_ORG_API_KEY is present in the process environment
    match std::env::var("OPENWEATHERMAP_ORG_API_KEY") {
        Ok(k) => println!("OPENWEATHERMAP_ORG_API_KEY is set (length {})", k.len()),
        Err(_) => println!("OPENWEATHERMAP_ORG_API_KEY is NOT set in this process environment"),
    }

    // Call the discovered capability 'openweather api' with a sample endpoint for London
    // The capability implementation will ensure the proper base and parameters.
    let expr = r#"(call "openweather api" "/data/2.5/weather?q=London")"#;
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
