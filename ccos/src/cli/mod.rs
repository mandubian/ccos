//! CCOS CLI module
//!
//! Unified command-line interface for all CCOS operations.
//!
//! # Commands
//!
//! - `discover` - Capability discovery (goal, server, search, inspect)
//! - `server` - Server management (list, add, remove, health)
//! - `approval` - Approval queue (pending, approve, reject, timeout)
//! - `call` - Execute a capability
//! - `plan` - Planning (create, execute, validate)
//! - `rtfs` - RTFS operations (eval, repl, run)
//! - `governance` - Governance (check, audit, constitution)
//! - `config` - Configuration (show, validate, init)

pub mod commands;
pub mod context;
pub mod output;

pub use context::CliContext;
pub use output::{OutputFormat, OutputFormatter};
