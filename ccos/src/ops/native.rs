//! Native capabilities operations
//!
//! This module contains operations for native capabilities (ccos.cli.*).

use crate::capabilities::provider::CapabilityProvider;
use crate::capability_marketplace::types::{
    CapabilityManifest, EffectType, NativeCapability, ProviderType,
};
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
                domains: vec!["cli".to_string()],
                categories: vec!["system".to_string()],
                effect_type,
                approval_status: crate::capability_marketplace::types::ApprovalStatus::Approved,
            };
            marketplace.register_capability_manifest(manifest).await?;
        }
    }

    Ok(())
}

/// Register all filesystem capabilities (ccos.fs.*) to the marketplace
pub async fn register_fs_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    use crate::capabilities::providers::LocalFileProvider;
    use std::sync::Arc;

    let fs_provider = Arc::new(LocalFileProvider::default());
    let descriptors = CapabilityProvider::list_capabilities(fs_provider.as_ref());

    for descriptor in descriptors {
        let (effects, mut effect_type) = get_capability_effects(&descriptor.id);

        let provider_clone = fs_provider.clone();
        let cap_id = descriptor.id.clone();

        // Refine effect_type based on user feedback: read operations are less risky if VM isolated
        let is_read = descriptor.id.contains("read")
            || descriptor.id.contains("list")
            || descriptor.id.contains("exists");
        if is_read {
            effect_type = EffectType::PureProvisional;
        }

        let mut metadata = descriptor.metadata.clone();
        metadata.insert(
            "risk_level".to_string(),
            if is_read {
                "medium".to_string()
            } else {
                "high".to_string()
            },
        );
        metadata.insert("isolation_requirement".to_string(), "microvm".to_string());

        let native_cap = NativeCapability {
            handler: Arc::new(move |inputs| {
                let provider = provider_clone.clone();
                let id = cap_id.clone();
                let inputs = inputs.clone();
                Box::pin(async move {
                    provider.execute_capability(
                        &id,
                        &inputs,
                        &crate::capabilities::provider::ExecutionContext::default(),
                    )
                })
            }),
            security_level: if is_read {
                "medium".to_string()
            } else {
                "high".to_string()
            },
            metadata: metadata.clone(),
        };

        let manifest = CapabilityManifest {
            id: descriptor.id.clone(),
            name: descriptor.id.clone(),
            description: descriptor.description.clone(),
            provider: ProviderType::Native(native_cap),
            version: "0.2.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: descriptor
                .security_requirements
                .permissions
                .iter()
                .map(|p| format!("{:?}", p))
                .collect(),
            effects,
            metadata,
            agent_metadata: None,
            domains: vec!["fs".to_string(), "filesystem".to_string()],
            categories: vec![if is_read {
                "crud.read".to_string()
            } else {
                "crud.write".to_string()
            }],
            effect_type,
            approval_status: crate::capability_marketplace::types::ApprovalStatus::Approved,
        };
        marketplace.register_capability_manifest(manifest).await?;
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
        // FS capabilities - effectful
        id if id.starts_with("ccos.fs.") => (vec!["fs".to_string()], EffectType::Effectful),
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
