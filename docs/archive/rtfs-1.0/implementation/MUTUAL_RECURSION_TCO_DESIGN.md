# Design for Mutual Recursion and Tail Call Optimization (TCO) in the RTFS IR Runtime

## 1. Problem

The RTFS compiler's IR runtime experienced stack overflow errors when executing mutually recursive functions. This was because each function call, even those in a tail position, would create a new stack frame, leading to unbounded stack growth for recursive patterns.

## 2. Goal

The primary goal was to support mutual recursion without causing stack overflows. This is achieved by implementing a robust Tail Call Optimization (TCO) mechanism that can handle both direct and mutual recursion.

## 3. Implementation Choices

The solution revolves around a few key components that work together to identify and optimize tail calls.

### 3.1. `is_tail` Flag

- **What it is:** A boolean flag, `is_tail`, is now passed through the evaluation functions of the IR runtime, including `execute_node`, `execute_if`, `execute_let`, and `execute_do`.
- **Purpose:** This flag indicates whether a given expression is in a "tail position." An expression is in a tail position if its result is the final result of the enclosing function, without any further computation.
- **How it works:**
    - The flag is initiated as `true` for the last expression in a function body.
    - Control flow constructs like `if`, `let`, and `do` propagate the flag to their own tail positions. For example, in an `(if condition then-branch else-branch)`, both `then-branch` and `else-branch` are in tail position if the `if` expression itself is.
    - For a function application `(f arg1 arg2)`, the call is only considered a tail call if the `is_tail` flag is `true`.

### 3.2. `RuntimeError::TailCall` Variant

- **What it is:** A new variant was added to the `RuntimeError` enum:
  ```rust
  TailCall {
      function: Value,
      args: Vec<Value>,
  }
  ```
- **Purpose:** Instead of performing a recursive function call directly from a tail position, the `execute_apply` function now returns this special `Err` variant. This acts as a signal to the calling context that a tail call needs to be performed.
- **Benefit:** This unwinds the Rust stack gracefully, preventing it from growing. The information required for the next call (the target function and its arguments) is neatly packaged in the `TailCall` variant.

### 3.3. TCO Loop in `call_ir_lambda`

- **What it is:** The `call_ir_lambda` function, which handles the execution of user-defined IR functions, is now built around a `'tco: loop`.
- **How it works:**
    1.  The function starts by setting up the environment for the current call.
    2.  It executes the expressions in the function body.
    3.  If the last expression is a tail call, `execute_apply` will return an `Err(RuntimeError::TailCall { ... })`.
    4.  `call_ir_lambda` catches this specific error. Instead of propagating it, it updates its own local variables (`params`, `body`, `closure_env`, `args`) with the details from the `TailCall` variant.
    5.  It then uses `continue 'tco;` to jump to the beginning of the loop, effectively reusing the current stack frame for the next function call.
    6.  If the function body completes without a `TailCall` error, the loop terminates, and the final result is returned normally.

### 3.4. `letrec` Semantics for Mutual Recursion

- **Problem:** For mutual recursion, functions defined within the same `let` block need to be able to "see" each other. A simple, sequential definition would fail.
- **Solution:** The `execute_let` function was implemented with `letrec` (recursive let) semantics using a multi-pass approach:
    1.  **Pass 1: Placeholders for Functions:** The runtime first iterates through the `let` bindings and identifies those whose initializers are `IrNode::Lambda`. For each of these, it defines a `Value::FunctionPlaceholder` in the new environment. This placeholder is a shared, mutable reference (`Rc<RefCell<Value>>`).
    2.  **Pass 2: Evaluate Non-Functions:** It then evaluates and defines all non-function bindings. These bindings can safely reference the function placeholders.
    3.  **Pass 3: Resolve Placeholders:** Finally, it evaluates the lambda bodies for the function bindings. The resulting `Value::Function` is then used to update the corresponding placeholder, making the actual function available to all other bindings in the scope.
- **`VariableRef` Resolution:** The logic for `IrNode::VariableRef` was updated to automatically resolve `FunctionPlaceholder` values before returning them, ensuring that callers always receive the final, concrete value.

## 4. Conclusion

This combination of an explicit `is_tail` flag, a dedicated `TailCall` error for signaling, a TCO loop, and a `letrec` implementation provides a comprehensive solution for safe and efficient execution of mutually recursive functions in the RTFS runtime. It avoids stack overflows by transforming recursive calls in tail positions into iterative jumps within the `call_ir_lambda` loop.
