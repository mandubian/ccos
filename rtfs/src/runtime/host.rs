//! Compatibility shim for CCOS RuntimeHost
//!
//! This module serves as a placeholder for `RuntimeHost` which is provided by CCOS when integrated.
//! For standalone RTFS execution, use `rtfs::runtime::pure_host::create_pure_host()` instead.
//!
//! This file exists to preserve import paths during migration.
//! The actual `RuntimeHost` implementation lives in `ccos::host::RuntimeHost`.
