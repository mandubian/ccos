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
                effects: vec![],
                metadata: descriptor.metadata.clone(),
                agent_metadata: None,
                domains: Vec::new(),
                categories: Vec::new(),
                effect_type: EffectType::default(),
            };
            marketplace.register_capability_manifest(manifest).await?;
        }
    }

    Ok(())
}
