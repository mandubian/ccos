// Compatibility shim for runtime::capability_marketplace to re-export the CCOS marketplace
// Re-export the CCOS capability marketplace *types* and MCP discovery only.
// We intentionally avoid re-exporting the `marketplace` module implementation
// as that defines methods that conflict with other `impl` blocks in the
// migration workspace. Call sites should use `CapabilityMarketplace` type
// from here and construct using the provided helper functions in the
// marketplace module when needed.
pub use crate::ccos::capability_marketplace::types::*;
pub use crate::ccos::capability_marketplace::mcp_discovery::*;

// Backward-compatible convenience wrappers expected by existing tests and examples.
// These delegate to the CCOS `CapabilityMarketplace` implementation in `types.rs`.
// We keep them small and non-invasive.
impl CapabilityMarketplace {
	/// Register an HTTP capability using the older convenience name.
	/// Builds a CapabilityManifest and delegates to `register_local_capability`.
	pub async fn register_http_capability(
		&self,
		id: String,
		name: String,
		description: String,
		base_url: String,
		auth_token: Option<String>,
	) -> Result<(), crate::runtime::error::RuntimeError> {
		let manifest = CapabilityManifest {
			id: id.clone(),
			name,
			description,
			provider: ProviderType::Http(HttpCapability { base_url, auth_token, timeout_ms: 5000 }),
			version: "1.0.0".to_string(),
			input_schema: None,
			output_schema: None,
			attestation: None,
			provenance: None,
			permissions: vec![],
			metadata: std::collections::HashMap::new(),
		};
		// Use the lower-level register_local_capability available on the type
		// Call the canonical CCOS impl unambiguously to avoid ambiguity with
		// compatibility wrapper names also present in this module.
		CapabilityMarketplace::register_local_capability(self, &id, manifest).await
	}

	/// Register a local capability with optional schema (convenience wrapper).
	pub async fn register_local_capability_with_schema(
		&self,
		id: String,
		name: String,
		description: String,
		handler: std::sync::Arc<dyn Fn(&crate::runtime::values::Value) -> crate::runtime::error::RuntimeResult<crate::runtime::values::Value> + Send + Sync>,
		input_schema: Option<crate::ast::TypeExpr>,
		output_schema: Option<crate::ast::TypeExpr>,
	) -> Result<(), crate::runtime::error::RuntimeError> {
		let manifest = CapabilityManifest {
			id: id.clone(),
			name,
			description,
			provider: ProviderType::Local(LocalCapability { handler }),
			version: "1.0.0".to_string(),
			input_schema,
			output_schema,
			attestation: None,
			provenance: None,
			permissions: vec![],
			metadata: std::collections::HashMap::new(),
		};
		CapabilityMarketplace::register_local_capability(self, &id, manifest).await
	}

	/// Legacy 4-argument registration API used across tests/examples.
	/// Provided as a compatibility wrapper with a different name to avoid
	/// colliding with the canonical `register_local_capability(&str, CapabilityManifest)`
	/// inherent method defined in the CCOS types module.
	pub async fn register_local_capability_legacy(
		&self,
		id: String,
		name: String,
		description: String,
		handler: std::sync::Arc<dyn Fn(&crate::runtime::values::Value) -> crate::runtime::error::RuntimeResult<crate::runtime::values::Value> + Send + Sync>,
	) -> Result<(), crate::runtime::error::RuntimeError> {
		let manifest = CapabilityManifest {
			id: id.clone(),
			name,
			description,
			provider: ProviderType::Local(LocalCapability { handler }),
			version: "1.0.0".to_string(),
			input_schema: None,
			output_schema: None,
			attestation: None,
			provenance: None,
			permissions: vec![],
			metadata: std::collections::HashMap::new(),
		};
		self.register_local_capability(&id, manifest).await
	}

	/// Execute with validation config: compatibility wrapper that converts params to a Value
	/// and forwards to the slice-based `execute_capability` implementation.
	pub async fn execute_with_validation_config(
		&self,
		capability_id: &str,
		params: &std::collections::HashMap<String, crate::runtime::values::Value>,
		_config: &crate::runtime::type_validator::TypeCheckingConfig,
	) -> Result<crate::runtime::values::Value, crate::runtime::error::RuntimeError> {
		// Convert params map to a single Value::Map and pass as single-arg slice.
		let mut map = std::collections::HashMap::new();
		for (k, v) in params.iter() {
			let key = if k.starts_with(':') { crate::ast::MapKey::Keyword(crate::ast::Keyword(k[1..].to_string())) } else { crate::ast::MapKey::String(k.clone()) };
			map.insert(key, v.clone());
		}
		let input_value = crate::runtime::values::Value::Map(map);
		let args = vec![input_value];
		self.execute_capability(capability_id, args.as_slice()).await
	}

}
