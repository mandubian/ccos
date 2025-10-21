//! Test program for multi-capability synthesis
//!
//! This program demonstrates the enhanced capability synthesizer that can generate
//! multiple specialized capabilities from a single API discovery.

use rtfs_compiler::ccos::synthesis::capability_synthesizer::{
    CapabilitySynthesizer, MultiCapabilityEndpoint, MultiCapabilitySynthesisRequest,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Testing Multi-Capability Synthesis System");
    println!("=============================================");

    // Create a mock synthesizer for testing
    let synthesizer = CapabilitySynthesizer::mock();

    // Create a multi-capability synthesis request for OpenWeatherMap API
    let request = MultiCapabilitySynthesisRequest {
        api_domain: "unifieddata".to_string(),
        api_docs: r#"
Unified Data Service API

Profile API:
- Endpoint: /v1/profile/{userId}
- Method: GET
- Description: Retrieve profile information for a given user identifier

Activity API:
- Endpoint: /v1/activity
- Method: POST
- Description: Submit an activity payload describing user interactions

Insights API:
- Endpoint: /v1/insights
- Method: GET
- Description: Fetch aggregated analytics insights for dashboards
        "#
        .to_string(),
        base_url: "https://api.unifieddata.example.com".to_string(),
        requires_auth: true,
        auth_provider: Some("unifieddata".to_string()),
        endpoints: vec![
            MultiCapabilityEndpoint {
                capability_suffix: "profile".to_string(),
                description: "Fetch user profile information".to_string(),
                path: "/v1/profile/{userId}".to_string(),
                http_method: Some("GET".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "userId": {"type": "string"},
                        "expand": {"type": "boolean"}
                    },
                    "required": ["userId"]
                })),
                output_schema: None,
            },
            MultiCapabilityEndpoint {
                capability_suffix: "activity".to_string(),
                description: "Record user activity events".to_string(),
                path: "/v1/activity".to_string(),
                http_method: Some("POST".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "events": {
                            "type": "array",
                            "items": {"type": "object"}
                        }
                    },
                    "required": ["events"]
                })),
                output_schema: None,
            },
            MultiCapabilityEndpoint {
                capability_suffix: "insights".to_string(),
                description: "Retrieve analytics insights aggregates".to_string(),
                path: "/v1/insights".to_string(),
                http_method: Some("GET".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "range": {"type": "string"},
                        "metric": {"type": "string"}
                    },
                    "required": ["range", "metric"]
                })),
                output_schema: None,
            },
        ],
        target_endpoints: None,
        generate_all_endpoints: false,
    };

    println!("📋 Synthesis Request:");
    println!("  API Domain: {}", request.api_domain);
    println!("  Base URL: {}", request.base_url);
    println!("  Requires Auth: {}", request.requires_auth);
    println!(
        "  Generate All Endpoints: {}",
        request.generate_all_endpoints
    );
    println!();

    // Generate the enhanced prompt
    let prompt = synthesizer.generate_multi_capability_prompt(&request);
    println!("🤖 Generated Multi-Capability Prompt:");
    println!("{}", prompt);
    println!();

    // Synthesize multiple capabilities
    println!("⚡ Synthesizing Multiple Capabilities...");
    let result = synthesizer.synthesize_multi_capabilities(&request).await?;

    println!("✅ Synthesis Complete!");
    println!("  Generated {} capabilities", result.capabilities.len());
    println!(
        "  Overall Quality Score: {:.2}",
        result.overall_quality_score
    );
    println!("  All Safety Passed: {}", result.all_safety_passed);
    println!();

    // Display each generated capability
    for (i, capability_result) in result.capabilities.iter().enumerate() {
        println!(
            "🔧 Capability {}: {}",
            i + 1,
            capability_result.capability.name
        );
        println!("   ID: {}", capability_result.capability.id);
        println!(
            "   Description: {}",
            capability_result.capability.description
        );
        println!("   Quality Score: {:.2}", capability_result.quality_score);
        println!("   Safety Passed: {}", capability_result.safety_passed);

        if let Some(input_schema) = &capability_result.capability.input_schema {
            println!(
                "   Input Schema: {}",
                serde_json::to_string_pretty(input_schema)?
            );
        }

        if let Some(output_schema) = &capability_result.capability.output_schema {
            println!(
                "   Output Schema: {}",
                serde_json::to_string_pretty(output_schema)?
            );
        }

        println!("   RTFS Implementation:");
        println!("   {}", capability_result.implementation_code);
        println!();
    }

    // Display common warnings
    if !result.common_warnings.is_empty() {
        println!("⚠️  Common Warnings:");
        for warning in &result.common_warnings {
            println!("   - {}", warning);
        }
        println!();
    }

    // Demonstrate the difference from single capability synthesis
    println!("🔄 Comparison with Single Capability Synthesis:");
    println!("   Single capability synthesis generates one generic HTTP wrapper");
    println!("   Multi-capability synthesis generates specialized, domain-specific capabilities");
    println!("   Each capability has its own input/output schema and RTFS implementation");
    println!();

    println!("🎯 Benefits of Multi-Capability Synthesis:");
    println!("   ✅ Type Safety - RTFS schemas ensure correct input/output");
    println!("   ✅ Better UX - Domain-specific function names and parameters");
    println!("   ✅ Validation - Automatic parameter validation");
    println!("   ✅ Documentation - Self-documenting capabilities");
    println!("   ✅ Composability - Mix and match different weather APIs");
    println!("   ✅ Pure RTFS - All logic generated in RTFS, no hardcoded Rust");
    println!();

    println!("🚀 Multi-Capability Synthesis Test Complete!");
    Ok(())
}
