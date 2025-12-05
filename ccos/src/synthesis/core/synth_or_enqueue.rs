//! Helpers to enqueue missing-capability placeholders when synthesis fails.
//!
//! This is intentionally small so planner/resolution code can call it without
//! pulling in the full synthesis pipeline. It only writes an artifact to the
//! synth queue for later human/LLM reification.

use std::path::PathBuf;

use rtfs::runtime::error::RuntimeResult;
use serde_json::Value;

use crate::synthesis::{SynthQueue, SynthQueueItem, SynthQueueStatus};

/// Enqueue a placeholder for a missing capability that could not be synthesized.
///
/// - `capability_id`: fully qualified id to be implemented
/// - `description`: natural language description of the intent/need
/// - `input_schema`/`output_schema`: optional JSON schemas
/// - `example_input`/`example_output`: optional examples to guide reification
/// - `source_intent`: optional intent text that triggered this enqueue
/// - `notes`: optional diagnostics or next-step hints
pub fn enqueue_missing_capability_placeholder(
    capability_id: impl Into<String>,
    description: impl Into<String>,
    input_schema: Option<Value>,
    output_schema: Option<Value>,
    example_input: Option<Value>,
    example_output: Option<Value>,
    source_intent: Option<String>,
    notes: Option<String>,
) -> RuntimeResult<PathBuf> {
    let mut item = SynthQueueItem::needs_impl(capability_id, description);
    item.input_schema = input_schema;
    item.output_schema = output_schema;
    item.example_input = example_input;
    item.example_output = example_output;
    item.source_intent = source_intent;
    item.notes = notes;
    item.status = SynthQueueStatus::NeedsImpl;

    let queue = SynthQueue::new(None);
    queue.enqueue(item)
}


