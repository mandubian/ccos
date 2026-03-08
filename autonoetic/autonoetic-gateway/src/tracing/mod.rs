//! Session Tracer - centralized session management and trace emission.
//!
//! This module provides a unified abstraction for managing session lifecycle,
//! event sequencing, and causal trace emission across the gateway.

pub mod session_tracer;

pub use session_tracer::*;
