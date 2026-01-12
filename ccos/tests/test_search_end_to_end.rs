use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::catalog::CatalogService;
use futures::future::BoxFuture;
use rtfs::ast::TypeExpr;
use rtfs::runtime::values::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_native_tool_discovery_end_to_end() {
    // 1. Initialize Marketplace (simulating ccos-mcp.rs setup)
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));

    // Initialize CatalogService and attach it to Marketplace
    let catalog_service = Arc::new(CatalogService::new());
    marketplace
        .set_catalog_service(catalog_service.clone())
        .await;

    // 2. Register a mock native tool (simulating register_ecosystem_tool)

    // Handler signature must match: Fn(&Value) -> BoxFuture<RuntimeResult<Value>>
    let handler = Arc::new(
        move |_arg: &Value| -> BoxFuture<'static, rtfs::runtime::error::RuntimeResult<Value>> {
            Box::pin(async move { Ok(Value::String("success".to_string())) })
        },
    );

    marketplace
        .register_native_capability_with_schema(
            "ccos_search".to_string(),         // id
            "ccos_search".to_string(),         // name
            "Search the codebase".to_string(), // description
            handler,
            "standard".to_string(), // security_level
            Some(TypeExpr::Any),    // input_schema (simplified)
            Some(TypeExpr::Any),    // output_schema
        )
        .await
        .expect("Failed to register tool");

    // Explicitly refresh catalog index to ensure new capability is indexed
    marketplace.refresh_catalog_index().await;

    // 3. Search for the tool using CatalogService via Marketplace
    // Note: get_catalog() returns Option<Arc<CatalogService>>
    let catalog = marketplace
        .get_catalog()
        .await
        .expect("Catalog service unavailable");

    // search_keyword(query, filter, limit)
    let results = catalog.search_keyword("search", None, 10).await;

    // 4. Verify discovery
    // results is Vec<CatalogHit>
    assert!(!results.is_empty(), "Should find at least one result");

    let found = results.iter().any(|h| h.entry.id == "ccos_search");
    assert!(found, "Should find ccos_search in results");
}
