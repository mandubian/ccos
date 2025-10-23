//! Test OpenWeather API Introspection
//!
//! This test demonstrates API introspection on the real OpenWeather API,
//! discovering endpoints and generating proper RTFS capability files
//! similar to the existing capability.rtfs file.

use rtfs_compiler::ccos::synthesis::capability_synthesizer::CapabilitySynthesizer;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌤️  Testing OpenWeather API Introspection");
    println!("=========================================");
    println!();

    // Create a mock synthesizer for testing
    let synthesizer = CapabilitySynthesizer::mock();

    // OpenWeather API details
    let api_url = "https://openweathermap.org/api";
    let api_domain = "openweather";

    println!("🔍 Introspecting OpenWeather API");
    println!("   URL: {}", api_url);
    println!("   Domain: {}", api_domain);
    println!();

    // Perform API introspection
    let introspection_result = synthesizer
        .synthesize_from_api_introspection(api_url, api_domain)
        .await?;

    println!("✅ API Introspection Complete!");
    println!("   Discovered {} capabilities", introspection_result.capabilities.len());
    println!("   Overall Quality Score: {:.2}", introspection_result.overall_quality_score);
    println!("   All Safety Passed: {}", introspection_result.all_safety_passed);
    println!();

    // Display discovered capabilities
    println!("📋 Discovered Capabilities:");
    println!("---------------------------");
    for (i, capability_result) in introspection_result.capabilities.iter().enumerate() {
        println!("{}. {} ({})", 
            i + 1,
            capability_result.capability.name,
            capability_result.capability.id
        );
        println!("   Description: {}", capability_result.capability.description);
        println!("   Endpoint: {} {}", 
            capability_result.capability.metadata.get("endpoint_method").unwrap_or(&"GET".to_string()),
            capability_result.capability.metadata.get("endpoint_path").unwrap_or(&"/".to_string())
        );
        
        // Show schemas
        if capability_result.capability.input_schema.is_some() {
            println!("   ✅ Has input schema");
        }
        if capability_result.capability.output_schema.is_some() {
            println!("   ✅ Has output schema");
        }
        println!();
    }

    // Save capabilities to RTFS files
    println!("💾 Saving Capabilities to RTFS Files");
    println!("-------------------------------------");
    
    // Save to /capabilities at project root (parent of rtfs_compiler)
    let output_dir = std::env::current_dir()?
        .parent()
        .map(|p| p.join("capabilities"))
        .unwrap_or_else(|| PathBuf::from("../capabilities"));
    
    let introspector = synthesizer.get_introspector();

    for capability_result in &introspection_result.capabilities {
        let file_path = introspector.save_capability_to_rtfs(
            &capability_result.capability,
            &capability_result.implementation_code,
            &output_dir,
        )?;

        println!("✅ Saved: {}", file_path.display());
    }
    println!();

    // Display RTFS content for each capability
    println!("📄 Generated RTFS Files:");
    println!("========================");
    println!();

    for (i, capability_result) in introspection_result.capabilities.iter().enumerate() {
        println!("📝 Capability {}: {}", i + 1, capability_result.capability.id);
        println!("{}", "=".repeat(80));
        
        let rtfs_content = introspector.capability_to_rtfs_string(
            &capability_result.capability,
            &capability_result.implementation_code,
        );
        
        println!("{}", rtfs_content);
        println!("{}", "=".repeat(80));
        println!();
    }

    println!("🎉 OpenWeather API Introspection Complete!");
    println!();
    println!("📁 Capability files saved to: capabilities/<capability-id>/capability.rtfs");
    println!("   You can now use these capabilities in your CCOS workflows!");

    Ok(())
}

