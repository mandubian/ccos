// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

pub mod evaluator;
pub mod stdlib;
pub mod values;
pub mod environment;
pub mod error;

pub use evaluator::Evaluator;
pub use values::Value;
pub use environment::Environment;
pub use error::{RuntimeError, RuntimeResult};
