# Mutation primitives inventory (test cases)

Date: 2025-09-19

This document lists `.rtfs` feature test cases that use mutation primitives (atoms, swap!, reset!, set!, deref/@) and suggested short-term actions.

Files scanned:
- `function_expressions.rtfs` (19 cases)
- `do_expressions.rtfs` (18 cases)
- `parallel_expressions.rtfs` (19 cases)
- `mutation_and_state.rtfs` (4 cases)

Detected mutation cases (index-based)

- function_expressions.rtfs
  - index 6: uses `set!` to mutate a captured `counter` in closure
    - snippet: (let [counter 0] (let [inc (fn [] (set! counter (+ counter 1)))] (do (inc) (inc) counter)))
    - recommendation: migrate to immutable pattern (returning new values) or mark expected-fail temporarily.
  - index 18: memoization using `atom`, `swap!` and `@`/deref on `cache`
    - snippet: memo-fib with `(let [cache (atom {})] ...)` and `(swap! cache assoc n result)`
    - recommendation: replace with a pure memoization helper in tests or keep as expected-fail until runtime migration.

- do_expressions.rtfs
  - index 7: iterative sum using `atom`, `reset!`, and `deref`
    - snippet: (do (def sum (atom 0)) (dotimes [i 5] (reset! sum (+ (deref sum) i))) (deref sum))
    - recommendation: rewrite using a pure loop/reduce where possible; otherwise mark expected-fail temporarily.

- parallel_expressions.rtfs
  - index 14: concurrent counter increments using `(atom 0)` and `swap!`
    - snippet: (let [counter (atom 0)] (parallel [inc1 (swap! counter inc)] ...))
    - recommendation: either remove concurrency-dependent mutation from tests or keep as expected-fail while rethinking concurrency model.
  - index 16: uses `atom` for logging and `swap!` inside parallel tasks
    - snippet: (let [log (atom [])] (parallel [task1 (do (swap! log conj "task1") 1)] ...))
    - recommendation: rewrite to return values from parallel tasks and assert composition; or mark expected-fail.

- mutation_and_state.rtfs
  - index 0: uses `set!` for rebind in frame
    - snippet: (let [x 1] (do (set! x 2) x))
    - recommendation: rewrite to use let shadowing or return mutated value immutably.
  - index 1: (atom + swap! + deref)
    - snippet: (let [c (atom 0) _ (swap! c inc)] (deref c))
    - recommendation: rewrite to return incremented value directly.
  - index 2: sequential atom updates to calculate sum
    - snippet: (let [sum (atom 0)] (do (swap! sum + 0) ... (deref sum)))
    - recommendation: rewrite using reduce over a range.
  - index 3: deref sugar test (@a)
    - snippet: (let [a (atom 42)] @a)
    - recommendation: simple literal or keep as expected-fail until migration.

Suggested short-term plan

1. Mark these specific test-case indices as expected-fail in the harness only if they surface under `--no-default-features --no-legacy-atoms` builds. (The harness already treats the runtime atom-removal error as expected.)
2. For each case, create a small migration task to rewrite the test as a pure/immutable expression where possible. Prioritize:
   - `mutation_and_state.rtfs` (indices 0..3) — small, self-contained; easy to rewrite.
   - `do_expressions.rtfs` index 7 — can likely be expressed with reduce.
   - `function_expressions.rtfs` index 6/18 and `parallel_expressions.rtfs` index 14/16 — larger refactors; consider keeping as expected-fail temporarily and opening tasks to migrate.

Next steps

- Create migration PRs for the easy cases (mutation_and_state cases, do_expressions[7]).
- Open follow-up issues for the heavier cases (memoization using atom, parallel mutation patterns).

If you want, I can create the migration branches and PRs for the easy rewrites now.
