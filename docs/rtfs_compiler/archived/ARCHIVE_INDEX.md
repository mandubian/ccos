<!-- AUTO-GENERATED ARCHIVE INDEX -->
# RTFS Compiler - Archived Documents

Date: 2025-08-18

This index lists the documents moved into this archive folder when the `rtfs_compiler/` root was decluttered. Each entry has a one-line summary to help you find historical implementation reports, completion notes, and planning artifacts.

If you want a file restored to `rtfs_compiler/`, run:

```bash
git mv docs/rtfs_compiler/archived/<FILENAME> rtfs_compiler/
git commit -m "docs(rtfs_compiler): restore <FILENAME> from archive"
```

---

Archived files (alpha order)

- `ARBITER_IMPLEMENTATION_PLAN.md` — Long-form implementation plan for the CCOS Arbiter (design and roadmaps).
- `CARGO_TOML_CLEANUP_SUMMARY.md` — Notes and actions taken to tidy Cargo.toml manifests across crates.
- `COMPILER_PROGRESS_REPORT.md` — Periodic progress report covering compiler milestones and status.
- `CONSOLIDATED_TESTING_ANALYSIS.md` — Aggregate analysis of testing results and flaky test triage.
- `CUDA_IMPLEMENTATION_SUMMARY.md` — Summary of optional CUDA-related implementation efforts and notes.
- `CUDA_OPTIONAL_GUIDE.md` — Guide for optional CUDA setup and usage (non-mandatory).
- `ISSUE_1_COMPLETION_REPORT.md` — Completion report for Issue #1 (intent persistence / storage work).
- `ISSUE_2_COMPLETION_REPORT.md` — Completion report for Issue #2 (intent graph relationships and features).
- `ISSUE_2_SUBGRAPH_COMPLETION_REPORT.md` — Subgraph export/restore completion report tied to Issue #2.
- `ISSUE_3_COMPLETION_REPORT.md` — Completion report for Issue #3 (spec/synthesis tasks).
- `ISSUE_4_COMPLETION_REPORT.md` — Completion report for Issue #4.
- `ISSUE_5_COMPLETION_REPORT.md` — Completion report for Issue #5.
- `ISSUE_6_COMPLETION_REPORT.md` — Completion report for Issue #6 (causal chain / working memory wiring).
- `ISSUE_6_CAUSAL_CHAIN_EVENT_STREAM_SPEC.md` — Spec for causal-chain event stream and WM ingestion adapter.
- `ISSUE_23_COMPLETION_REPORT.md` — Completion report for Arbiter V1 (NL→intent/plan + delegation bootstraps).
- `ISSUE_42_COMPLETION_REPORT.md` — Completion report for Issue #42.
- `ISSUE_41_COMPLETION_REPORT.md` — Completion report for Issue #41.
- `ISSUE_43_COMPLETION_SUMMARY.md` — Summary for Issue #43 (capability system stabilization) and related outcomes.
- `ISSUE_43_IMPLEMENTATION_PLAN.md` — Implementation plan used for Issue #43 workstreams.
- `ISSUE_50_COMPLETION_REPORT.md` — Completion report for Issue #50 (type system / plan parsing enhancements).
- `ISSUE_52_COMPLETION_REPORT.md` — Completion report for Issue #52 (language stability & stdlib work).
- `ISSUE_53_UNIT_TEST_STABILIZATION.md` — Unit test stabilization follow-up notes.
- `ISSUE_55_CCOS_RTFS_LIBRARY.md` — Notes and plan for CCOS RTFS library work (Intent Graph bindings et al.).
- `ISSUE_79_COMPLETION_REPORT.md` — Completion report for Issue #79 (execution contexts & checkpointing).
- `ISSUE_X_UNIFIED_STORAGE_COMPLETION_REPORT.md` — Variant/comparison report around unified storage completion.
- `MCP_IMPLEMENTATION_SUMMARY.md` — Implementation summary for MCP (Model Context Protocol) integration.
- `REMOTE_MODELS_IMPLEMENTATION_SUMMARY.md` — Summary of remote model integration efforts.
- `REMOTE_MODELS_GUIDE.md` — User/developer guide for remote model usage (kept historically).
- `UNIFIED_STORAGE_COMPLETION_REPORT.md` — Report for the unified storage workstream.
- `HIERARCHICAL_CONTEXT_IMPLEMENTATION.md` — Implementation notes on hierarchical execution contexts and ContextManager.

---

Notes
- This index is intentionally lightweight. For deeper context, open the archived file; each completion report contains links to PRs, commits, and tests used for verification.
- If you prefer a different archive location (e.g., `docs/archive/`), I can move the files and update this index accordingly.

Maintainers: if you add more archived files, please append them to this index with a short description.
