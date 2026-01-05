//! Configuration types and parsing for RTFS
//!
//! **NOTE**: Agent runtime configuration types (`AgentConfig` and related types) have been
//! migrated to the CCOS crate (`ccos::config::types`) as part of issue #166. The types in this
//! module are kept for backwards compatibility but are deprecated. When using CCOS, import from
//! `ccos::config::types` instead.

pub mod parser;
pub mod profile_selection;
pub mod self_programming_session;
pub mod types;
pub mod validation;

pub use parser::AgentConfigParser;
pub use profile_selection::*;
// Note: Types are deprecated in favor of CCOS config module (issue #166)
// We can't use #[deprecated] here because RTFS can't reference CCOS
pub use types::*;
pub use validation::*;
