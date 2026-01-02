//! Planner foundations.
//!
//! This module exposes reusable building blocks for the smart assistant planner
//! so they can evolve into first-class RTFS capabilities.

pub mod adapters;
pub mod capabilities;
pub mod capabilities_v2;
pub mod catalog_adapter;
pub mod coverage;
pub mod dialogue_planner;
pub mod menu;
pub mod modular_planner;
pub mod resolution;
pub mod signals;

pub use catalog_adapter::CcosCatalogAdapter;
pub use dialogue_planner::DialoguePlanner;
