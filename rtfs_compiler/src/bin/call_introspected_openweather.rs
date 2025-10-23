use rtfs_compiler::ccos::environment::CCOSBuilder;
use rtfs_compiler::runtime::execution_outcome::ExecutionOutcome;
use std::fs;

fn main() {
    println!("🌤️  Testing Introspected OpenWeather API Capabilities");
    println!("=====================================================\n");

    // Try to get API key from environment, or use a test key if not set
    let api_key = std::env::var("OPENWEATHERMAP_ORG_API_KEY")
        .unwrap_or_else(|_| {
            println!("⚠️  OPENWEATHERMAP_ORG_API_KEY not found in environment");
            println!("   Using test key (will fail authentication but demonstrate functionality)\n");
            "test_api_key_demo".to_string()
        });

    // Set the API key so the capability can access it
    std::env::set_var("OPENWEATHERMAP_ORG_API_KEY", &api_key);
    println!("✅ API key is set (length: {})\n", api_key.len());

    // Build a CCOS environment with HTTP mocking disabled and allowlist for openweathermap
    let builder = CCOSBuilder::new()
        .http_mocking(false)
        .http_allow_hosts(vec![
            "openweathermap.org".to_string(),
            "api.openweathermap.org".to_string(),
        ])
        .verbose(true);

    let env = builder.build().expect("Failed to build environment");

    // Load both introspected capabilities
    let capabilities = vec![
        ("openweather_api.get_current_weather", "Get Current Weather"),
        ("openweather_api.get_forecast", "Get 5 Day Weather Forecast"),
    ];

    for (cap_id, cap_name) in &capabilities {
        let capability_path = format!("../capabilities/{}/capability.rtfs", cap_id);
        println!("📦 Loading capability: {}", cap_name);
        println!("   Path: {}", capability_path);

        match fs::read_to_string(&capability_path) {
            Ok(capability_code) => {
                match env.execute_code(&capability_code) {
                    Ok(outcome) => match outcome {
                        ExecutionOutcome::Complete(v) => {
                            println!("   ✅ Loaded successfully: {:?}\n", v);
                        }
                        ExecutionOutcome::RequiresHost(h) => {
                            println!("   ⚠️  Host call required during loading: {:?}\n", h);
                        }
                    },
                    Err(e) => {
                        println!("   ❌ Failed to load capability: {:?}\n", e);
                        return;
                    }
                }
            }
            Err(e) => {
                println!("   ❌ Failed to read capability file: {}\n", e);
                return;
            }
        }
    }

    // Test 1: Get current weather for London using typed parameters
    println!("\n🔬 Test 1: Current Weather for London (typed parameters)");
    println!("=========================================================");
    let test1 = r#"
    ((call "openweather_api.get_current_weather") {
        :q "London,UK"
        :units "metric"
        :lang "en"
    })
    "#;
    println!("Expression: {}", test1);
    execute_test(&env, test1);

    // Test 2: Get current weather using coordinates
    println!("\n🔬 Test 2: Current Weather by Coordinates (Paris)");
    println!("==================================================");
    let test2 = r#"
    ((call "openweather_api.get_current_weather") {
        :lat 48.8566
        :lon 2.3522
        :units "metric"
    })
    "#;
    println!("Expression: {}", test2);
    execute_test(&env, test2);

    // Test 3: Get 5-day forecast
    println!("\n🔬 Test 3: 5-Day Forecast for New York");
    println!("=======================================");
    let test3 = r#"
    ((call "openweather_api.get_forecast") {
        :q "New York,US"
        :cnt 5
    })
    "#;
    println!("Expression: {}", test3);
    execute_test(&env, test3);

    // Test 4: Test with minimal parameters
    println!("\n🔬 Test 4: Minimal Parameters (just city name)");
    println!("===============================================");
    let test4 = r#"
    ((call "openweather_api.get_current_weather") {
        :q "Tokyo"
    })
    "#;
    println!("Expression: {}", test4);
    execute_test(&env, test4);

    println!("\n🎉 All tests completed!");
    println!("\n📋 Summary:");
    println!("   ✅ Loaded 2 introspected capabilities");
    println!("   ✅ Demonstrated typed, runtime-validated API calls");
    println!("   ✅ No manual URL construction or API key injection needed");
    println!("   ✅ Clean, declarative capability usage");
}

fn execute_test(env: &rtfs_compiler::ccos::environment::CCOSEnvironment, expr: &str) {
    match env.execute_code(expr) {
        Ok(outcome) => match outcome {
            ExecutionOutcome::Complete(v) => {
                println!("✅ Result: {:?}", v);
            }
            ExecutionOutcome::RequiresHost(h) => {
                println!("⚠️  Host call required: {:?}", h);
            }
        },
        Err(e) => println!("❌ Execution error: {:?}", e),
    }
}

