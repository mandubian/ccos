// Test agent unification functionality in the capability marketplace
// This demonstrates how agents are now capabilities with metadata flags

use rtfs_compiler::ccos::capability_marketplace::types::*;
use rtfs_compiler::CapabilityMarketplace;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_agent_unification_basic() -> Result<(), Box<dyn std::error::Error>> {
    // Create a marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::new(registry);
    
    // Register a primitive capability
    let primitive_manifest = CapabilityManifest::new(
        "math.add".to_string(),
        "Add Numbers".to_string(),
        "Adds two numbers together".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(rtfs_compiler::runtime::values::Value::Integer(42))),
        }),
        "1.0.0".to_string(),
    );
    
    // Register an agent capability
    let agent_manifest = CapabilityManifest::new_agent(
        "travel.trip-planner.agent.v1".to_string(),
        "Trip Planner Agent".to_string(),
        "Goal-directed trip planner with planning and interaction capabilities".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(rtfs_compiler::runtime::values::Value::String("Trip planned!".to_string()))),
        }),
        "1.0.0".to_string(),
        true,  // planning
        true,  // stateful
        true,  // interactive
    );
    
    // Register capabilities
    marketplace.register_capability_manifest(primitive_manifest).await?;
    marketplace.register_capability_manifest(agent_manifest).await?;
    
    // Test basic functionality
    let all_caps = marketplace.list_capabilities().await;
    println!("Found {} capabilities: {:?}", all_caps.len(), all_caps.iter().map(|c| &c.id).collect::<Vec<_>>());
    assert_eq!(all_caps.len(), 2);
    
    // Test filtering by kind
    let agents = marketplace.list_agents().await;
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "travel.trip-planner.agent.v1");
    assert!(agents[0].is_agent());
    assert!(agents[0].can_plan());
    assert!(agents[0].is_stateful());
    assert!(agents[0].is_interactive());
    
    let primitives = marketplace.list_primitives().await;
    assert_eq!(primitives.len(), 1);
    assert_eq!(primitives[0].id, "math.add");
    assert!(!primitives[0].is_agent());
    assert!(!primitives[0].can_plan());
    assert!(!primitives[0].is_stateful());
    assert!(!primitives[0].is_interactive());
    
    Ok(())
}

#[tokio::test]
async fn test_capability_query_filtering() -> Result<(), Box<dyn std::error::Error>> {
    // Create a marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::new(registry);
    
    // Register different types of capabilities
    let primitive = CapabilityManifest::new(
        "math.add".to_string(),
        "Add Numbers".to_string(),
        "Adds two numbers".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(rtfs_compiler::runtime::values::Value::Integer(42))),
        }),
        "1.0.0".to_string(),
    );
    
    let planning_agent = CapabilityManifest::new_agent(
        "planning.agent.v1".to_string(),
        "Planning Agent".to_string(),
        "Agent with planning capabilities".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(rtfs_compiler::runtime::values::Value::String("Planned!".to_string()))),
        }),
        "1.0.0".to_string(),
        true,  // planning
        false, // stateful
        false, // interactive
    );
    
    let interactive_agent = CapabilityManifest::new_agent(
        "interactive.agent.v1".to_string(),
        "Interactive Agent".to_string(),
        "Agent with interaction capabilities".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(rtfs_compiler::runtime::values::Value::String("Interactive!".to_string()))),
        }),
        "1.0.0".to_string(),
        false, // planning
        false, // stateful
        true,  // interactive
    );
    
    // Register all capabilities
    marketplace.register_capability_manifest(primitive).await?;
    marketplace.register_capability_manifest(planning_agent).await?;
    marketplace.register_capability_manifest(interactive_agent).await?;
    
    // Test query filtering
    let planning_query = CapabilityQuery::new().with_planning(true);
    let planning_caps = marketplace.list_capabilities_with_query(&planning_query).await;
    assert_eq!(planning_caps.len(), 1);
    assert_eq!(planning_caps[0].id, "planning.agent.v1");
    
    let interactive_query = CapabilityQuery::new().with_interactive(true);
    let interactive_caps = marketplace.list_capabilities_with_query(&interactive_query).await;
    assert_eq!(interactive_caps.len(), 1);
    assert_eq!(interactive_caps[0].id, "interactive.agent.v1");
    
    let agent_query = CapabilityQuery::new().agents_only();
    let agent_caps = marketplace.list_capabilities_with_query(&agent_query).await;
    assert_eq!(agent_caps.len(), 2);
    
    let primitive_query = CapabilityQuery::new().primitives_only();
    let primitive_caps = marketplace.list_capabilities_with_query(&primitive_query).await;
    assert_eq!(primitive_caps.len(), 1);
    assert_eq!(primitive_caps[0].id, "math.add");
    
    Ok(())
}

#[tokio::test]
async fn test_agent_metadata_helpers() -> Result<(), Box<dyn std::error::Error>> {
    // Test agent metadata helper methods
    let agent = CapabilityManifest::new_agent(
        "test.agent.v1".to_string(),
        "Test Agent".to_string(),
        "Test agent with all capabilities".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(rtfs_compiler::runtime::values::Value::String("Test".to_string()))),
        }),
        "1.0.0".to_string(),
        true,  // planning
        true,  // stateful
        true,  // interactive
    );
    
    // Test kind detection
    assert_eq!(agent.kind(), CapabilityKind::Agent);
    assert!(agent.is_agent());
    
    // Test capability flags
    assert!(agent.can_plan());
    assert!(agent.is_stateful());
    assert!(agent.is_interactive());
    
    // Test primitive capability
    let primitive = CapabilityManifest::new(
        "test.primitive".to_string(),
        "Test Primitive".to_string(),
        "Test primitive capability".to_string(),
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(rtfs_compiler::runtime::values::Value::Integer(1))),
        }),
        "1.0.0".to_string(),
    );
    
    assert_eq!(primitive.kind(), CapabilityKind::Primitive);
    assert!(!primitive.is_agent());
    assert!(!primitive.can_plan());
    assert!(!primitive.is_stateful());
    assert!(!primitive.is_interactive());
    
    Ok(())
}
