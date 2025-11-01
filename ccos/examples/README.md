# CCOS Examples

This directory contains working examples demonstrating CCOS+RTFS integration after the architectural migration.

## Overview

**12 working examples** showcasing different aspects of the CCOS architecture. All examples compile successfully ✅

## Active Examples (All Compile Successfully ✅)

### Smart Assistants & User Interaction (2 examples)

- `smart_assistant_demo.rs` (124KB) - **Comprehensive smart assistant**
  - Governed smart assistant with arbitrary natural-language goals
  - Delegating arbiter integration
  - Multi-turn clarifying questions
  - Capability discovery and matching
  - Plan generation and execution
  - Complete end-to-end workflow

- `user_interaction_progressive_graph.rs` (89KB) - **Progressive intent building**
  - Interactive conversation loops
  - Intent graph visualization
  - Capability synthesis
  - Real-time graph updates
  - User-driven refinement

### Core Integration (2 examples)

- `ccos_demo.rs` (1.4KB) - Minimal baseline example
  - Basic CCOS initialization
  - Simple plan execution
  - Quick start reference

- `comprehensive_demo.rs` (26KB) - Complete end-to-end pipeline
  - Natural language → Intent → Plan → Execution
  - MCP capability discovery
  - Full CCOS+RTFS workflow

### UI & User Interaction (1 example)

- `ccos_tui_demo.rs` (9KB) - Terminal UI demonstration
  - Interactive TUI interface
  - Real-time status updates
  - Event streaming visualization

### Runtime & Infrastructure (2 examples)

- `ccos_runtime_service_demo.rs` (5KB) - Runtime service embedding
  - Service API usage
  - Event subscription
  - Command sending patterns

- `rtfs_capability_demo.rs` (9KB) - RTFS capability persistence
  - Plan execution
  - Capability state management

### Capability System (1 example)

- `execute_capability.rs` (7KB) - Capability execution patterns
  - Direct capability calls
  - Input/output handling
  - Error scenarios

### Context Management (2 examples)

- `hierarchical_context_demo.rs` (10KB) - Hierarchical context handling
  - Context nesting
  - Isolation levels
  - Permission propagation

- `context_types_demo.rs` (5KB) - Context type system
  - RuntimeContext usage
  - ExecutionContext patterns
  - SecurityContext configuration

### Arbiter & LLM (1 example)

- `llm_arbiter_example.rs` (7KB) - LLM-based arbiter usage
  - Delegation configuration
  - LLM model integration
  - Plan generation via LLM

### Debugging Tools (1 example)

- `dump_intents.rs` (7KB) - Intent inspection tool
  - Intent graph traversal
  - Intent export/analysis
  - Debugging aid

## Archived Examples

**Complex examples** requiring more extensive migration work have been moved to `ccos/examples/archived/`:

- `github_mcp_demo.rs` (9KB) - GitHub MCP integration (needs provider updates)
- `intent_graph_demo.rs` (20KB) - Intent graph operations (needs API updates)
- `rtfs_reentrance_demo.rs` (7KB) - Re-entrant execution (needs checkpoint updates)
- `missing_capability_resolution_examples.rs` (18KB) - Capability resolution (needs synthesis updates)

Plus 30+ others preserved from the original `rtfs_compiler` package.

## Running Examples

### Basic Usage

```bash
# Run minimal example
cargo run --package ccos --example ccos_demo

# Run smart assistant (requires config)
cargo run --package ccos --example smart_assistant_demo -- \
  --config ../config/agent_config.toml \
  --goal "Plan a 2-day trip to Paris"

# Run progressive intent graph
cargo run --package ccos --example user_interaction_progressive_graph -- \
  --enable-delegation

# Run comprehensive demo
cargo run --package ccos --example comprehensive_demo -- --interactive

# Run TUI demo
cargo run --package ccos --example ccos_tui_demo
```

### Build All Examples

```bash
cargo build --package ccos --examples
```

All 12 active examples should compile successfully ✅

## Migration Notes

### What Changed

After the RTFS/CCOS architectural separation:
- Import paths updated: `rtfs_compiler::*` → `rtfs::*` or `ccos::*`
- Config moved: `ccos::config` → `rtfs::config`
- Many old APIs no longer exist or have changed significantly

### Examples Fixed

The following major examples were successfully migrated:
- ✅ `smart_assistant_demo.rs` (124KB) - Full smart assistant workflow
- ✅ `user_interaction_progressive_graph.rs` (89KB) - Progressive intent building
- ✅ `comprehensive_demo.rs` (26KB) - End-to-end pipeline

**Key fixes applied**:
- `use ccos::config::*` → `use rtfs::config::*`
- `rtfs_compiler::ccos::types::*` → `ccos::types::*`
- `rtfs_compiler::runtime::*` → `rtfs::runtime::*`
- Removed references to deprecated APIs

### Why Some Examples Were Archived

Examples were archived if they:
- Required APIs that were removed or significantly restructured
- Needed extensive rewrites beyond simple import updates
- Used old capability/MCP infrastructure that no longer exists
- Had complex dependencies on archived modules

All archived examples are preserved for historical reference and can be migrated incrementally as needed.

## Example Development

To create new examples:

1. Use the active examples as templates
2. Import from `ccos::*` for CCOS components
3. Import from `rtfs::*` for RTFS components
4. Use `rtfs::config` for configuration (not `ccos::config`)
5. Test compilation with `cargo build --package ccos --example your_example`

## Statistics

| Metric | Value |
|--------|-------|
| Active examples | 12 files |
| Total active size | ~360KB |
| Largest example | smart_assistant_demo.rs (124KB) |
| Archived examples | 34+ files |
| Compilation status | ✅ 100% passing (12/12) |

## Migrated From

These examples were migrated and fixed from the `rtfs_compiler` package.

**Original location**: `rtfs_compiler/examples/`  
**Migration date**: 2025-11-01  
**Fixes completed**: 2025-11-01 (all major examples now working)
