# RTFS Compiler and Runtime Finalization Plan

This document outlines the necessary work to bring the RTFS compiler and runtime to a stable, production-ready state. Each section represents a proposed GitHub issue, categorized by the area of the compiler stack.

**Progress Status**: 5/9 Issues Completed (55.6%) ✅

---

## 1. Parser & AST ✅ **COMPLETED**

### Issue #1: Enhance Parser Error Reporting ✅ **COMPLETED**

- **Title:** [Parser] Enhance Parser Error Reporting for Production Readiness
- **Status:** ✅ **COMPLETED** 
- **Description:** The current parser identifies syntax errors, but the error messages are often generic. For a production environment, developers need precise, context-aware error messages that pinpoint the exact location and suggest a fix.
- **Acceptance Criteria:**
    - [x] Errors for mismatched delimiters ( `(`, `[`, `{` ) specify the location of the opening delimiter.
    - [x] Invalid function call syntax provides suggestions (e.g., "Did you mean `(function-name arg1 arg2)`?").
    - [x] Errors in special forms (`let`, `if`, `fn`, etc.) are specific to the form's syntax.
    - [x] The `parser_error_reporter.rs` is enhanced to provide contextual snippets in error messages.
- **Labels:** `compiler`, `parser`, `enhancement`

### Issue #2: Full Grammar-to-AST Coverage Test ✅ **COMPLETED**

- **Title:** [Parser] Implement Full Grammar-to-AST Coverage Test Suite
- **Status:** ✅ **COMPLETED**
- **Description:** While many parts of the grammar are parsed, there is no systematic test to ensure every single rule in `rtfs.pest` is correctly mapped to a corresponding structure in `ast.rs`.
- **Acceptance Criteria:**
    - [x] Create a new test file `tests/ast_coverage.rs`.
    - [x] For each rule in `rtfs.pest`, create at least one valid RTFS snippet that uses it.
    - [x] The test will parse the snippet and assert that the correct AST node is generated.
    - [x] This includes all literals, type expressions, special forms, and top-level definitions.
- **Labels:** `compiler`, `parser`, `testing`

---

## 2. Intermediate Representation (IR) & Optimization ✅ **COMPLETED**

### Issue #3: Audit and Complete IR for All Language Features ✅ **COMPLETED**

- **Title:** [IR] Audit and Complete IR for All Language Features
- **Status:** ✅ **COMPLETED** 🎉
- **Description:** The IR needs to be audited to ensure it can represent 100% of the language features defined in the AST, including complex pattern matching, advanced type expressions, and all special forms.
- **Acceptance Criteria:**
    - [x] Review `ir::converter.rs` and ensure every `ast::Expression` variant has a corresponding IR representation.
    - [x] Implement IR generation for any missing features (e.g., complex `match` expressions, `with-resource`).
    - [x] Create integration tests that convert feature-specific ASTs to IR and validate the output.
- **Achievement:** 15 comprehensive test functions validating 100% language feature coverage
- **Labels:** `compiler`, `ir`, `enhancement`

### Issue #4: Implement and Test IR Optimization Passes ✅ **COMPLETED**

- **Title:** [IR] Implement and Test Core IR Optimization Passes
- **Status:** ✅ **COMPLETED** 🎉
- **Description:** The `optimizer.rs` and `enhanced_optimizer.rs` files exist, but a suite of standard optimization passes should be implemented and tested, such as constant folding, dead code elimination, and function inlining.
- **Acceptance Criteria:**
    - [x] Implement constant folding for arithmetic and boolean operations.
    - [x] Implement dead code elimination for `let` bindings that are never used.
    - [x] Add a test suite in `tests/ir_optimization.rs` that provides unoptimized IR and asserts that the optimized IR is correct and more efficient.
- **Achievement:** Comprehensive IR optimization system with 7 test cases validating constant folding, dead code elimination, and control flow optimization
- **Dependencies:** ✅ Issue #3 completed (prerequisite satisfied)
- **Labels:** `compiler`, `ir`, `performance`

---

## 3. Runtime & Execution 🔥 **NEXT PRIORITY**

### Issue #5: Stabilize and Secure the Capability System

- **Title:** [Runtime] Stabilize and Secure the Capability System
- **Status:** 🔄 **PENDING**
- **Description:** The capability system is a core feature but needs to be hardened for production. This includes finalizing advanced providers, implementing dynamic discovery, and ensuring security.
- **Acceptance Criteria:**
    - [ ] Implement the remaining provider types from the CCOS tracker: MCP, A2A, Plugins, RemoteRTFS.
    - [ ] Implement dynamic capability discovery (e.g., via a network registry).
    - [ ] Add input/output schema validation for all capability calls.
    - [ ] Implement capability attestation and provenance checks in the runtime.
- **Labels:** `runtime`, `capability`, `security`

### Issue #6: End-to-End Testing for All Standard Library Functions

- **Title:** [Runtime] Create End-to-End Tests for All Standard Library Functions
- **Status:** 🔄 **PENDING**
- **Description:** Every function available in the standard library (`stdlib.rs` and `secure_stdlib.rs`) must have a corresponding end-to-end test that executes it via the RTFS runtime.
- **Acceptance Criteria:**
    - [ ] Create a directory `rtfs_compiler/tests/stdlib/`.
    - [ ] For each stdlib function, create an `.rtfs` file that calls it with valid arguments.
    - [ ] Create a test that runs the file and asserts the correct output.
    - [ ] Include tests for edge cases and invalid arguments to test error handling.
- **Labels:** `runtime`, `stdlib`, `testing`

---

## 4. Comprehensive Testing ⏳ **PENDING**

### Issue #7: Create End-to-End Grammar Feature Test Matrix

- **Title:** [Testing] Create End-to-End Grammar Feature Test Matrix
- **Status:** 🔄 **PENDING**
- **Description:** This is the most critical task for stabilization. We need a systematic, end-to-end test for every single feature in the language, from parsing to execution.
- **Acceptance Criteria:**
    - [ ] Create a new test file `tests/e2e_features.rs`.
    - [ ] Create a directory `rtfs_compiler/tests/rtfs_files/features/`.
    - [ ] For every major grammar rule and special form (`let`, `if`, `fn`, `match`, `try/catch`, `parallel`, etc.), create a dedicated `.rtfs` file.
    - [ ] Each file should contain multiple tests for that feature, including happy paths and edge cases.
    - [ ] The test runner will execute each file and assert the final result is correct.
- **Labels:** `testing`, `compiler`, `runtime`, `epic`

### Issue #8: Implement Fuzz Testing for the Parser

- **Title:** [Testing] Implement Fuzz Testing for the Parser
- **Status:** 🔄 **PENDING**
- **Description:** To ensure the parser is robust against unexpected input and to find panics, we should implement fuzz testing.
- **Acceptance Criteria:**
    - [ ] Integrate a fuzzing library like `cargo-fuzz`.
    - [ ] Create a fuzz target for the main `parse_program` function.
    - [ ] Run the fuzzer to identify and fix any panics or crashes.
- **Labels:** `testing`, `parser`, `security`

---

## 5. Documentation ⏳ **PENDING**

### Issue #9: Write Formal RTFS Language Specification

- **Title:** [Docs] Write Formal RTFS Language Specification
- **Status:** 🔄 **PENDING**
- **Description:** The project needs a formal specification document that details the syntax, semantics, and standard library of the RTFS language. This will be the source of truth for implementers and users.
- **Acceptance Criteria:**
    - [ ] Create a new document `docs/rtfs-spec.md`.
    - [ ] Document the full grammar, with examples for each rule.
    - [ ] Document the behavior of all special forms.
    - [ ] Document the full standard library, including function signatures and descriptions.
- **Labels:** `documentation`

---

## 4. Comprehensive Testing

### Issue #7: Create End-to-End Grammar Feature Test Matrix

- **Title:** [Testing] Create End-to-End Grammar Feature Test Matrix
- **Description:** This is the most critical task for stabilization. We need a systematic, end-to-end test for every single feature in the language, from parsing to execution.
- **Acceptance Criteria:**
    - [ ] Create a new test file `tests/e2e_features.rs`.
    - [ ] Create a directory `rtfs_compiler/tests/rtfs_files/features/`.
    - [ ] For every major grammar rule and special form (`let`, `if`, `fn`, `match`, `try/catch`, `parallel`, etc.), create a dedicated `.rtfs` file.
    - [ ] Each file should contain multiple tests for that feature, including happy paths and edge cases.
    - [ ] The test runner will execute each file and assert the final result is correct.
- **Labels:** `testing`, `compiler`, `runtime`, `epic`

### Issue #8: Implement Fuzz Testing for the Parser

- **Title:** [Testing] Implement Fuzz Testing for the Parser
- **Description:** To ensure the parser is robust against unexpected input and to find panics, we should implement fuzz testing.
- **Acceptance Criteria:**
    - [ ] Integrate a fuzzing library like `cargo-fuzz`.
    - [ ] Create a fuzz target for the main `parse_program` function.
    - [ ] Run the fuzzer to identify and fix any panics or crashes.
- **Labels:** `testing`, `parser`, `security`

---

## 5. Documentation

### Issue #9: Write Formal RTFS Language Specification

- **Title:** [Docs] Write Formal RTFS Language Specification
- **Description:** The project needs a formal specification document that details the syntax, semantics, and standard library of the RTFS language. This will be the source of truth for implementers and users.
- **Acceptance Criteria:**
    - [ ] Create a new document `docs/rtfs-spec.md`.
    - [ ] Document the full grammar, with examples for each rule.
    - [ ] Document the behavior of all special forms.
    - [ ] Document the full standard library, including function signatures and descriptions.
- **Labels:** `documentation`
