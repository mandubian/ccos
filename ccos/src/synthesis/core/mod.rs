//! Core resolution and synthesis components.
//!
//! This module contains the most critical resolution infrastructure:
//! - Missing capability resolution and strategies
//! - Schema serialization for RTFS types
//! - Feature flags for capability resolution
//! - Dependency extraction from generated artifacts

pub mod dependency_extractor;
pub mod feature_flags;
pub mod missing_capability_resolver;
pub mod missing_capability_strategies;
pub mod schema_serializer;

// Re-export commonly used types
pub use feature_flags::{FeatureFlagChecker, MissingCapabilityConfig, MissingCapabilityFeatureFlags};
pub use missing_capability_resolver::{
    MissingCapabilityRequest, MissingCapabilityResolver, ResolutionResult,
};
pub use missing_capability_strategies::MissingCapabilityStrategy;
pub use schema_serializer::type_expr_to_rtfs_compact;
