//! Secrets module for secure credential management
//!
//! Provides layered secret storage with:
//! - Local project secrets (.ccos/secrets.toml) - for exportable standalone plans
//! - Environment variable fallback - global user scope

mod secret_store;

pub use secret_store::SecretStore;
