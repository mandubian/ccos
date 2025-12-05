# Plan for Implementing `defmacro` in RTFS

This document outlines the plan to implement the `defmacro` feature in RTFS. The implementation will be broken down into three main phases.

## Phase 1: Parsing and Storing Macros

The first step is to teach the compiler to recognize and store macro definitions.

- [ ] **Update the Parser (`rtfs/src/rtfs.pest`)**:
    - [ ] Add `defmacro` to the list of recognized keywords or special forms.
- [ ] **Create a `Macro` Struct**:
    - [ ] In a new file, `rtfs/src/compiler/macro.rs`, define a struct to represent a compiled macro. This struct will hold the macro's name, its parameter list, and its body (as an unevaluated AST node).
- [ ] **Modify the `Compiler` State**:
    - [ ] Add a `HashMap<String, Macro>` to the `Compiler` struct to serve as the macro table.
- [ ] **Implement `compile_defmacro`**:
    - [ ] Create a new method in the compiler that handles the `defmacro` special form. This method will parse the macro's definition and store it in the macro table.

## Phase 2: The Macro Expansion Engine

This is the core of the implementation, where the AST is transformed.

- [ ] **Create a Macro Expander (`expander.rs`)**:
    - [ ] Create a new module, `rtfs/src/compiler/expander.rs`.
    - [ ] The main function will be `expand(node: &Value, macros: &HashMap<String, Macro>) -> Result<Value, ExpansionError>`.
- [ ] **Implement the Expansion Logic**:
    - [ ] Recursively traverse the AST.
    - [ ] If a macro call is found, expand it by substituting parameters into the macro body.
    - [ ] Handle `quasiquote` (`'`), `unquote` (`~`), and `unquote-splicing` (`~@`).
    - [ ] Recursively expand the result of a macro expansion to allow macros to expand into other macros.

## Phase 3: Integration and Testing

Finally, we'll integrate this new expansion pass into the compiler and add tests.

- [ ] **Integrate the Expansion Pass**:
    - [ ] In the main `Compiler::compile` function, add a call to the `expand` function after parsing and before IR generation or evaluation.
- [ ] **Write Comprehensive Tests**:
    - [ ] Create a new test file, `rtfs/tests/test_macros.rs`.
    - [ ] Add tests for:
        - [ ] Simple macro definition and expansion (e.g., `(when ...)`).
        - [ ] Correct handling of `quasiquote`, `unquote`, and `unquote-splicing`.
        - [ ] Recursive expansion (macros expanding into other macros).
        - [ ] Hygienic concerns (if applicable).
        - [ ] Error conditions (e.g., wrong number of arguments).
