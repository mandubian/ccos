# Plan: Semantic Plan Judge Integration

**Status**: Draft
**Date**: 2025-12-30
**Target**: Governance Kernel

## Overview

This plan introduces a "Semantic Plan Judge" into the `GovernanceKernel`. This component acts as a final "common sense" check, using an LLM to verify that a generated plan is semantically sound and safe with respect to the user's goal.

This addresses the "Semantic Gap" where a plan might be technically valid (allowed by policy) but semantically nonsensical or dangerous (e.g., resolving "delete files" to a "discovery" tool because of loose embedding associations).

## Implementation Plan

### 1. Create `PlanJudge` Component
**File**: `ccos/src/governance_judge.rs`

Create a new module responsible for semantic evaluation.
- **Struct**: `PlanJudge`
- **Dependencies**: `LlmProvider` (for judgment), `GovernanceConfig` (for settings).
- **Method**: `judge_plan(goal: &str, plan: &Plan, resolutions: &HashMap<String, String>) -> Result<Judgment, RuntimeError>`

**Judgment Logic**:
The LLM prompt will evaluate:
1.  **Goal Alignment**: Does the plan actually achieve the stated goal?
2.  **Semantic Safety**: Are the tools appropriate for the action? (e.g., flagging "delete" goals mapped to "read-only" tools).
3.  **Hallucination Check**: Does the plan invent parameters or steps that don't make sense for the resolved capabilities?

### 2. Register Module
**File**: `ccos/src/lib.rs`

- Add `pub mod governance_judge;` to expose the new module.

### 3. Integrate into Governance Kernel
**File**: `ccos/src/governance_kernel.rs`

- **Struct Update**: Add `plan_judge: PlanJudge` to the `GovernanceKernel` struct.
- **Initialization**: Update `GovernanceKernel::new` to initialize the judge.
- **Pipeline Update**: In `validate_and_execute`, insert the judge step:
    1.  Sanitize Intent
    2.  Scaffold Plan
    3.  Constitution Validation (Policy)
    4.  **-> Semantic Judgment (New Step) <-**
    5.  Execution Mode Validation
    6.  Execution

### 4. Configuration
**File**: `ccos/src/governance_kernel.rs` (or config module)

- Add configuration to toggle the judge (default: enabled).
- Define "Fail-Open" vs "Fail-Closed" behavior (Default: Fail-Open with warning if LLM is down, Fail-Closed if LLM rejects).

## Example Scenario: "Delete vs Discover"

**Goal**: "Delete files in /tmp"
**Bad Resolution**: `cli.discover` (matched via "file system" embedding)
**Current Behavior**: Allowed (if `cli.discover` is whitelisted).
**New Behavior**:
- **Judge**: "The goal is destructive ('delete'), but the plan uses a discovery tool ('cli.discover'). This is a semantic mismatch."
- **Action**: Block execution and return a `GovernanceDenial`.

## Future Work
- **Feedback Loop**: Feed the judge's rejection reason back to the `ModularPlanner` for auto-repair.
- **Risk Scoring**: Have the judge assign a risk score (0-1) instead of binary Allow/Deny.
