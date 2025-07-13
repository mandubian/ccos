# RTFS 2.0 – Standard Library Extensions

**Status:** Draft (July 2025)

RTFS 2.0 inherits all RTFS 1.0 core built-in functions. This document tracks _additions_ and _behavioural changes_ introduced by the Cognitive Computing Operating System (CCOS) layer.

---

## 1. Human-in-the-Loop Helper

### 1.1. `(ask-human prompt [expected-type])`

|                |                                                           |
| -------------- | --------------------------------------------------------- |
| **Namespace**  | Core builtin (no `tool:` prefix)                          |
| **Capability** | `ccos.ask-human` (logged)                                 |
| **Signature**  | `[:=> [:cat :string :keyword?] [:resource PromptTicket]]` |
| **Returns**    | `#resource-handle("prompt-…")`                            |

**Arguments**

1. `prompt` _(string, required)_ – Question shown to the user.
2. `expected-type` _(keyword, optional)_ – Hint for UI validation. Common values: `:text`, `:number`, `:boolean`, `:choice`.

**Behaviour**

1. Generates a prompt ticket (`prompt-<uuid>`). Execution _may_ proceed or suspend – orchestration is left to the Arbiter.
2. Emits a `CapabilityCall` action to the **Causal Chain** so auditors can trace user interactions.
3. No stdout/stderr side-effects – UI integration happens at Arbiter level via `issue_user_prompt` / `resolve_user_prompt`.

**Error Conditions**

- `:error/arity-mismatch` – No prompt provided.
- `:error/type` – `prompt` not string or `expected-type` not keyword.

**Rationale**
The helper provides a language-level primitive for human feedback without polluting the evaluator with blocking I/O. It fits naturally into the capability model and provenance ledger.

---

_More extensions will be documented in this file as CCOS evolves._
