//! Native capabilities operations
//!
//! This module contains operations for native capabilities (ccos.cli.*).

use crate::capabilities::provider::CapabilityProvider;
use crate::capability_marketplace::types::{CapabilityManifest, EffectType, ProviderType};
use crate::capability_marketplace::CapabilityMarketplace;
use rtfs::runtime::error::RuntimeResult;

/// Register all native capabilities (ccos.cli.*) to the marketplace
pub async fn register_native_capabilities(
    marketplace: &CapabilityMarketplace,
) -> RuntimeResult<()> {
    // Register native capabilities (ccos.cli.*)
    // We instantiate the provider to get the descriptors and implementations
    let native_provider = crate::capabilities::NativeCapabilityProvider::new();
    let descriptors = CapabilityProvider::list_capabilities(&native_provider);

    for descriptor in descriptors {
        if let Some(native_cap) = native_provider.get_capability(&descriptor.id) {
            // Determine effects based on capability type
            let (effects, effect_type) = get_capability_effects(&descriptor.id);

            let manifest = CapabilityManifest {
                id: descriptor.id.clone(),
                name: descriptor.id.clone(),
                description: descriptor.description.clone(),
                provider: ProviderType::Native(native_cap.clone()),
                version: "1.0.0".to_string(),
                input_schema: None,  // TODO: Extract from descriptor
                output_schema: None, // TODO: Extract from descriptor
                attestation: None,
                provenance: None,
                permissions: descriptor
                    .security_requirements
                    .permissions
                    .iter()
                    .map(|p| format!("{:?}", p))
                    .collect(),
                effects,
                metadata: descriptor.metadata.clone(),
                agent_metadata: None,
                domains: Vec::new(),
                categories: Vec::new(),
                effect_type,
            };
            marketplace.register_capability_manifest(manifest).await?;
        }
    }

    Ok(())
}

/// Determine effects for a capability based on its ID
fn get_capability_effects(id: &str) -> (Vec<String>, EffectType) {
    match id {
        // LLM capabilities - effectful but deterministic for same input
        "ccos.llm.generate" => (
            vec!["llm".to_string(), "network".to_string()],
            EffectType::Effectful,
        ),
        // IO capabilities - effectful
        id if id.starts_with("ccos.io.") || id.starts_with("ccos.cli.") => {
            (vec!["io".to_string()], EffectType::Effectful)
        }
        // Config capabilities - read-only effectful
        id if id.contains("config") || id.contains("show") => {
            (vec!["read".to_string()], EffectType::Effectful)
        }
        // Default - pure
        _ => (vec![], EffectType::Pure),
    }
}
