# User Interaction Patterns: using ccos.user.ask

This document demonstrates patterns for human-in-the-loop interaction using the `ccos.user.ask` capability in RTFS 2.0.

## Overview

The `ccos.user.ask` capability enables RTFS programs to interact with end-users in real-time. The CCOS Host mediates this interaction (typically via stdin/stdout or a UI), ensuring all prompts and responses are governed and recorded in the Causal Chain.

## Capability: ccos.user.ask

**Signature:**
```clojure
(call "ccos.user.ask" prompt-string)
```

**Arguments:**
- `prompt-string`: The question or prompt to display to the user.

**Returns:** String value with the user's response.

**Security:** Requires `:io` and `:user-interaction` permissions.

## Pattern 1: Basic User Input

**Goal:** Ask a single question and use the answer.

```clojure
(let [name (call "ccos.user.ask" "What is your name? ")]
  (call "ccos.echo" (str "Hello, " name "! Nice to meet you.")))
```

**Use Cases:**
- Simple data collection
- Configuration prompts
- User confirmation dialogs

## Pattern 2: Multi-Step Data Collection

**Goal:** Collect multiple pieces of information sequentially using a single `let` block.

```clojure
(let [name (call "ccos.user.ask" "Your name: ")
      age (call "ccos.user.ask" "Your age: ")
      hobby (call "ccos.user.ask" "Your hobby: ")]
  (call "ccos.echo" (str "Thank you, " name ". You are " age " and enjoy " hobby ".")))
```

**Key Insight:** Sequential bindings in a single `let` keep all previous answers in scope, making summary logic easy to write.

## Pattern 3: Conditional Branching

**Goal:** Use conditional logic to create different execution paths based on user choices.

### Binary Choice (if)

```clojure
(let [likes (call "ccos.user.ask" "Do you like pizza? (yes/no)")]
  (if (= likes "yes")
    (call "ccos.echo" "Great! Pizza is delicious!")
    (call "ccos.echo" "Maybe try it sometime!")))
```

### Multiple Choice (match)

For 3+ options, `match` is the preferred pattern.

```clojure
(let [lang (call "ccos.user.ask" "Choose a language: rust, python, or javascript")]
  (match lang
    "rust" (call "ccos.echo" "Safe and fast!")
    "python" (call "ccos.echo" "Simple and elegant!")
    "javascript" (call "ccos.echo" "The language of the web!")
    _ (call "ccos.echo" "An interesting choice!")))
```

## Scoping and Persistence

### Lexical Scoping
`let` bindings are **lexically scoped**. Variables defined in a `let` block are only available within that block.

```clojure
;; ✅ CORRECT: Usage is inside the let body
(let [answer (call "ccos.user.ask" "Continue?")]
  (if (= answer "yes") :ok :cancel))

;; ❌ INCORRECT: Variable used outside its scope
(let [answer (call "ccos.user.ask" "Name?")]
  answer)
(call "ccos.echo" answer) ; Error: 'answer' is not defined here
```

## Security and Governance

User interaction is governed by CCOS policy. The Host will:
1. Verify the agent has `ccos.user.ask` in its capability allowlist.
2. Check for interactive approval requirements (some systems may require human approval *to even ask* a question).
3. Record both the prompt and the response in the **Causal Chain** for audit purposes.

---
*Note: This doc replaces the legacy USER_INTERACTION_PATTERNS.md which used obsolete 'step' syntax.*
