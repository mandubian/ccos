use ccos::ops::browser_discovery::BrowserDiscoveryService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let url = if args.len() > 1 {
        &args[1]
    } else {
        "https://petstore.swagger.io/"
    };

    let service = BrowserDiscoveryService::new();

    let llm_service = ccos::discovery::llm_discovery::LlmDiscoveryService::new().await?;

    println!(
        "🔍 Testing browser extraction with LLM analysis on {}...",
        url
    );
    match service.extract_with_llm_analysis(url, &llm_service).await {
        Ok(result) => {
            println!("✅ Extraction success: {}", result.success);
            if let Some(spec_url) = result.spec_url {
                println!("📦 Found spec_url: {}", spec_url);
            } else {
                println!("❌ No spec_url found");
            }
            println!("🔗 Found {} OpenAPI links", result.found_openapi_urls.len());
            for link in &result.found_openapi_urls {
                println!("  - {}", link);
            }
            println!("📡 Found {} endpoints", result.discovered_endpoints.len());
            for (i, ep) in result.discovered_endpoints.iter().take(5).enumerate() {
                println!("  {}. {} {}", i + 1, ep.method, ep.path);
            }
            if let Some(auth) = result.auth {
                println!("🔑 Found Auth Config: {:?}", auth);
            } else {
                println!("❌ No Auth Config found");
            }
            if let Some(err) = result.error {
                println!("⚠️ Error: {}", err);
            }
        }
        Err(e) => {
            println!("🔴 Failed to call service: {}", e);
        }
    }

    Ok(())
}
