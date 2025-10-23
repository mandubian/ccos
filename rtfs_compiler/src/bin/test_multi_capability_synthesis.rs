//! Test program for multi-capability synthesis with API introspection
//!
//! This program demonstrates the enhanced capability synthesizer that can:
//! 1. Introspect APIs to discover endpoints and schemas
//! 2. Encode API schemas in capability input_schema and output_schema fields
//! 3. Move controls to runtime rather than hardcoding them in implementations

use rtfs_compiler::ccos::synthesis::capability_synthesizer::{
    CapabilitySynthesizer, MultiCapabilityEndpoint, MultiCapabilitySynthesisRequest,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Testing Multi-Capability Synthesis with API Introspection");
    println!("=============================================================");

    // Create a mock synthesizer for testing
    let synthesizer = CapabilitySynthesizer::mock();

    // Test 1: API Introspection (NEW APPROACH)
    println!("\n🔍 Test 1: API Introspection");
    println!("----------------------------");
    
    let api_url = "https://api.unifieddata.example.com";
    let api_domain = "unifieddata";
    
    println!("Introspecting API: {}", api_url);
    let introspection_result = synthesizer
        .synthesize_from_api_introspection(api_url, api_domain)
        .await?;

    println!("✅ API Introspection Complete!");
    println!("  Discovered {} capabilities", introspection_result.capabilities.len());
    println!(
        "  Overall Quality Score: {:.2}",
        introspection_result.overall_quality_score
    );
    println!("  All Safety Passed: {}", introspection_result.all_safety_passed);
    println!();

    // Display introspected capabilities with proper schema encoding
    for (i, capability_result) in introspection_result.capabilities.iter().enumerate() {
        println!(
            "🔧 Introspected Capability {}: {}",
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

        // Show that schemas are properly encoded in the capability
        if let Some(input_schema) = &capability_result.capability.input_schema {
            println!("   ✅ Input Schema: {:?}", input_schema);
        } else {
            println!("   ❌ No Input Schema");
        }

        if let Some(output_schema) = &capability_result.capability.output_schema {
            println!("   ✅ Output Schema: {:?}", output_schema);
        } else {
            println!("   ❌ No Output Schema");
        }

        println!("   Runtime-Controlled Implementation:");
        println!("   {}", capability_result.implementation_code);
        println!();
    }

    // Demonstrate RTFS serialization
    println!("\n📄 RTFS Serialization");
    println!("---------------------");
    println!("Each capability is saved as a separate capability.rtfs file:");
    println!();

    for (i, capability_result) in introspection_result.capabilities.iter().enumerate() {
        println!(
            "📝 Capability {}: {} → {}/capability.rtfs",
            i + 1,
            capability_result.capability.id,
            capability_result.capability.id
        );

        // Get the introspector instance (recreate for serialization)
        let introspector = synthesizer.get_introspector();
        
        // Serialize to RTFS string
        let rtfs_content = introspector.capability_to_rtfs_string(
            &capability_result.capability,
            &capability_result.implementation_code,
        );

        println!("\n{}", "=".repeat(80));
        println!("{}", rtfs_content);
        println!("{}", "=".repeat(80));
        println!();
    }

    // Test 2: Legacy Hardcoded Approach (for comparison)
    println!("\n📋 Test 2: Legacy Hardcoded Approach (for comparison)");
    println!("----------------------------------------------------");
    
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

    println!("📋 Legacy Synthesis Request:");
    println!("  API Domain: {}", request.api_domain);
    println!("  Base URL: {}", request.base_url);
    println!("  Requires Auth: {}", request.requires_auth);
    println!(
        "  Generate All Endpoints: {}",
        request.generate_all_endpoints
    );
    println!();

    // Synthesize multiple capabilities using legacy approach
    println!("⚡ Synthesizing Multiple Capabilities (Legacy)...");
    let result = synthesizer.synthesize_multi_capabilities(&request).await?;

    println!("✅ Legacy Synthesis Complete!");
    println!("  Generated {} capabilities", result.capabilities.len());
    println!(
        "  Overall Quality Score: {:.2}",
        result.overall_quality_score
    );
    println!("  All Safety Passed: {}", result.all_safety_passed);
    println!();

    // Display each generated capability (legacy approach)
    for (i, capability_result) in result.capabilities.iter().enumerate() {
        println!(
            "🔧 Legacy Capability {}: {}",
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

        // Show that schemas are NOT properly encoded in legacy approach
        if let Some(input_schema) = &capability_result.capability.input_schema {
            println!("   ❌ Input Schema: {:?} (hardcoded)", input_schema);
        } else {
            println!("   ❌ No Input Schema");
        }

        if let Some(output_schema) = &capability_result.capability.output_schema {
            println!("   ❌ Output Schema: {:?} (hardcoded)", output_schema);
        } else {
            println!("   ❌ No Output Schema");
        }

        println!("   Hardcoded Implementation:");
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

    // Show the key differences
    println!("🔄 Key Differences: API Introspection vs Legacy Approach");
    println!("========================================================");
    println!();
    println!("✅ API Introspection Approach:");
    println!("   🔍 Discovers endpoints automatically from OpenAPI specs");
    println!("   📋 Encodes schemas in capability.input_schema and output_schema");
    println!("   🎛️  Moves controls to runtime (validation, auth, rate limiting)");
    println!("   🛡️  Runtime handles security and governance");
    println!("   🔧 Generates clean, runtime-controlled implementations");
    println!();
    println!("❌ Legacy Hardcoded Approach:");
    println!("   📝 Requires manual endpoint specification");
    println!("   🔧 Hardcodes schemas in implementation code");
    println!("   🎛️  Embeds controls directly in capability implementation");
    println!("   🚫 Mixes business logic with control logic");
    println!("   🔧 Generates complex, hardcoded implementations");
    println!();

    println!("🎯 Benefits of API Introspection Approach:");
    println!("   ✅ Automatic Discovery - No manual endpoint specification needed");
    println!("   ✅ Schema Encoding - Proper input/output schemas in capabilities");
    println!("   ✅ Runtime Controls - Validation, auth, rate limiting handled by runtime");
    println!("   ✅ Clean Separation - Business logic separate from control logic");
    println!("   ✅ Type Safety - RTFS schemas ensure correct input/output");
    println!("   ✅ Better UX - Domain-specific function names and parameters");
    println!("   ✅ Documentation - Self-documenting capabilities");
    println!("   ✅ Composability - Mix and match different APIs");
    println!("   ✅ Pure RTFS - All logic generated in RTFS, no hardcoded Rust");
    println!();

    println!("🚀 Multi-Capability Synthesis with API Introspection Test Complete!");
    Ok(())
}
