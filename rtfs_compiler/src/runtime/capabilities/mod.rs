// Compatibility shim: re-export capability provider interfaces from ccos::capabilities
// Historically these lived under `runtime::capabilities::provider`. During the
// CCOS/RTFS decoupling they were moved to `ccos::capabilities::provider`. Many
// provider implementations and tests still import the old path. This module
// provides thin re-exports to preserve the public API during migration.

pub mod provider;
pub mod providers;
pub mod registry;

// Keep convenience re-exports for code that imports `crate::runtime::capabilities::*`.
pub use provider::*;
pub use providers::*;
pub use registry::*;
