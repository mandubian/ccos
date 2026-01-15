use ccos::ops::browser_discovery::{BrowserDiscoveryResult, DiscoveredEndpoint};
use ccos::ops::introspection_service::{
    IntrospectionResult, IntrospectionService, IntrospectionSource,
};
use ccos::synthesis::introspection::{AuthConfig, AuthType};

#[tokio::test]
async fn test_browser_discovery_auth_generation() {
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().to_path_buf();

    // Mock an IntrospectionResult from Browser with Auth
    let auth_config = AuthConfig {
        auth_type: AuthType::ApiKey,
        provider: "test_provider".to_string(),
        key_location: Some("header".to_string()),
        in_header: Some(true),
        header_name: Some("X-Test-Key".to_string()),
        header_prefix: None,
        username_param: None,
        password_param: None,
        env_var: Some("TEST_API_KEY".to_string()),
        required: true,
        is_secret: true,
    };

    let browser_result = BrowserDiscoveryResult {
        success: true,
        source_url: "https://docs.example.com".to_string(), // Distinct from API url
        page_title: Some("Test API".to_string()),
        extracted_html: None,
        discovered_endpoints: vec![DiscoveredEndpoint {
            method: "GET".to_string(),
            path: "/data".to_string(),
            description: Some("Get data".to_string()),
            input_schema: None,
            output_schema: None,
            parameters: vec![],
        }],
        found_openapi_urls: vec![],
        spec_url: None,
        api_base_url: Some("https://api.example.com".to_string()),
        mcp_server_command: None,
        auth: Some(auth_config),
        error: None,
    };

    let result = IntrospectionResult {
        success: true,
        source: IntrospectionSource::Browser,
        server_name: "test-server".to_string(),
        api_result: None,
        browser_result: Some(browser_result),
        manifests: vec![],
        approval_id: None,
        error: None,
    };

    let service = IntrospectionService::empty();

    // Generate RTFS files
    let gen_result = service
        .generate_rtfs_files(&result, &output_path, "https://docs.example.com")
        .unwrap();

    // Verify server.rtfs content
    let server_rtfs = std::fs::read_to_string(gen_result.server_json_path).unwrap();
    println!("Generated server.rtfs:\n{}", server_rtfs);

    // It should contain the auth env var
    // Verify server.rtfs content - check for base_url
    assert!(server_rtfs.contains(":base_url \"https://api.example.com\""));
    assert!(server_rtfs.contains(":endpoint \"https://api.example.com\""));
    // Verify it contains the source URL in spec_url and description
    assert!(server_rtfs.contains(":spec_url \"https://docs.example.com\""));
    assert!(server_rtfs.contains("Discovered via browser from https://docs.example.com"));

    // Verify correct auth env var
    assert!(
        server_rtfs.contains(":auth_env_var \"TEST_API_KEY\""),
        "server.rtfs missing auth env var"
    );

    // Verify capability file content
    let cap_path = output_path.join("openapi").join("data.rtfs"); // Browser discovery puts files in openapi/ by default or similar logic
    let cap_rtfs = std::fs::read_to_string(cap_path).unwrap();
    println!("Generated capability rtfs:\n{}", cap_rtfs);

    // It should contain auth metadata
    assert!(
        cap_rtfs.contains(":auth {"),
        "Capability RTFS missing :auth block"
    );
    assert!(
        cap_rtfs.contains(":type \"api_key\""),
        "Capability RTFS missing auth type"
    );
    assert!(
        cap_rtfs.contains(":param_name \"X-Test-Key\""),
        "Capability RTFS missing param name"
    );
    assert!(
        cap_rtfs.contains(":env_var \"TEST_API_KEY\""),
        "Capability RTFS missing env var"
    );
}
