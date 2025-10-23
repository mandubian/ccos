use rtfs_compiler::ccos::environment::CCOSBuilder;
use rtfs_compiler::runtime::execution_outcome::ExecutionOutcome;
use std::fs;

fn main() {
    println!("🌤️  Demo: Introspected OpenWeather Capabilities with Mock API Key");
    println!("==================================================================\n");

    // Set a mock API key in the environment
    std::env::set_var("OPENWEATHERMAP_ORG_API_KEY", "mock_api_key_123456");
    println!("✅ Set OPENWEATHERMAP_ORG_API_KEY=mock_api_key_123456\n");

    // Build a CCOS environment with HTTP mocking disabled
    let builder = CCOSBuilder::new()
        .http_mocking(false)
        .http_allow_hosts(vec![
            "openweathermap.org".to_string(),
            "api.openweathermap.org".to_string(),
        ])
        .verbose(true);

    let env = builder.build().expect("Failed to build environment");

    // Load the introspected capability
    let capability_path = "../capabilities/openweather_api.get_current_weather/capability.rtfs";
    println!("📦 Loading capability from: {}", capability_path);

    match fs::read_to_string(capability_path) {
        Ok(capability_code) => {
            match env.execute_code(&capability_code) {
                Ok(outcome) => match outcome {
                    ExecutionOutcome::Complete(v) => {
                        println!("   ✅ Capability loaded: {:?}\n", v);
                    }
                    ExecutionOutcome::RequiresHost(h) => {
                        println!("   ⚠️  Host call required: {:?}\n", h);
                    }
                },
                Err(e) => {
                    println!("   ❌ Failed to load: {:?}\n", e);
                    return;
                }
            }
        }
        Err(e) => {
            println!("   ❌ Failed to read file: {}\n", e);
            return;
        }
    }

    // Test calling the capability
    println!("🔬 Test: Get Current Weather for London");
    println!("=========================================");
    let test_expr = r#"
    ((call "openweather_api.get_current_weather") {
        :q "London,UK"
        :units "metric"
    })
    "#;
    println!("Expression: {}\n", test_expr);

    match env.execute_code(test_expr) {
        Ok(outcome) => match outcome {
            ExecutionOutcome::Complete(v) => {
                println!("✅ Result received!");
                println!("{:?}", v);
            }
            ExecutionOutcome::RequiresHost(h) => {
                println!("⚠️  Host call required: {:?}", h);
            }
        },
        Err(e) => println!("❌ Execution error: {:?}", e),
    }

    println!("\n📊 Summary:");
    println!("   The URL should contain 'appid=mock_api_key_123456' in the debug output above.");
    println!("   Even though it's a mock key, this demonstrates that the API key injection works!");
}

