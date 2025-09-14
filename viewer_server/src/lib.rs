//! Library facade for viewer_server exposing testable helpers.
//!
//! The snapshot logic is factored into its own module so it can be shared by
//! both the binary (`main.rs`) and integration tests without compiling the
//! entire server twice.

pub mod snapshot;
pub use snapshot::build_architecture_snapshot;
