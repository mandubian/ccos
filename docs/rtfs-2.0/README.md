# RTFS: Reason about The Functional Spec

Welcome to **RTFS**, a pure functional language designed specifically for the era of Large Language Models (LLMs) and autonomous agents.

The name is a play on the classic "RTFM" (Read The Fucking Manual): it stands for **Reason about The Functional Spec** (or *Reason about The Fucking Spec*, depending on how much debugging you've had to do).

RTFS serves as the **universal data and logic carrier** for the **CCOS** (Cognitive Operating System). It provides a secure, homoiconic format that allows logic to be **executed within the CCOS environment** or **exchanged with other autonomous agents**. This enables LLMs to encode complex reasoning, define multi-step tasks, and manage execution flows in a portable, governed manner.

---

## 🎯 What is RTFS?

RTFS is not just another programming language; it is a **planning and execution language**. 

In a traditional OS, humans write code that the computer executes. In CCOS, an LLM often generates "plans" or "workflows." RTFS is the language of those plans. It is designed to be:
1.  **LLM-Native**: Its syntax (S-expressions) and structure are optimized for LLMs to generate and manipulate.
2.  **Pure & Secure**: RTFS code itself has no side effects. It cannot access your files or the internet directly. 
3.  **Host-Governed**: All actions affecting the real world (API calls, file writes, etc.) must be requested from a **Host** (like CCOS), which enforces security and governance policies.
4.  **Audit-Ready**: Every step of an RTFS execution creates a "Causal Chain," allowing for perfect auditing and transparency of an agent's reasoning process.

---

## 💡 Core Concepts in 3 Minutes

### 1. Everything is a Value
RTFS handles standard types: integers, strings, booleans, and lists. But it also treats **code as data**.
```clojure
;; A simple list of numbers
(1 2 3)

;; A piece of code (also a list!)
(+ 1 2)
```

### 2. The Host Boundary
This is the most important concept in RTFS. When RTFS needs to "do" something in the real world, it uses a **Request** mechanism.
```clojure
;; RTFS says: "Host, please execute this MCP tool"
(call :weather.fetch {:city "Paris"})
```
The RTFS interpreter pauses, the Host (CCOS) checks if the agent has permission, executes the tool, and then provides the **Response** back to RTFS to continue execution.

### 3. Intent-Driven Flows
RTFS is built to fulfill "Intents." An intent is a high-level goal (e.g., "Plan a trip to Tokyo"). RTFS breaks this down into steps, managing the flow between them, handling errors, and synthesizing results.

### 4. Safety via Gradual Typing
RTFS allows LLMs to be imprecise where it's easy, but strict where it matters. It supports **gradual typing** with refinement types.
```clojure
;; A function that enforces input types
(defn add-numbers [x :int y :int] 
  (+ x y))
```

---

## 🗺️ Navigation

Ready to dive deeper?

### 📑 [Language & Architecture Specifications](specs/README.md)
Detailed technical documentation on the language grammar, type system, IR compilation, and security model. This is where you'll find the **Implementation Status** for every feature.

### 📖 [Guides & Tutorials](guides/)
Practical "How-To" guides:
- [REPL Guide](guides/repl-guide.md): Get started with the interactive environment.
- [Type Checking](guides/type-checking-guide.md): Learn how to use RTFS's advanced type system.
- [Plan Generation](guides/plan-generation-guide.md): Guide for generating RTFS plans (for LLMs/Devs).
- [Context Variables](guides/context-variables.md): Using cross-plan context in workflows.
- [User Interaction](guides/user-interaction-patterns.md): Using `ccos.user.ask` in RTFS programs.
- [MCP Introspection](guides/mcp-introspection-demo.md): Discovering and registering MCP tools.
- [Streaming Basics](guides/streaming-basics.md): (Experimental) Understanding how RTFS handles real-time data flows.

### 🛠️ Core Specs Index
- [Philosophy & Design Goals](specs/00-philosophy.md)
- [Language Overview](specs/01-language-overview.md)
- [Syntax & Grammar](specs/02-syntax-and-grammar.md)
- [Standard Library](specs/10-standard-library.md)

---

## 🚀 Getting Started

If you have the CCOS repository cloned, you can start experimenting with RTFS immediately using the REPL:

```bash
cd rtfs_compiler/
cargo run --bin rtfs-repl
```

Try typing:
```clojure
rtfs> (+ 1 2 3)
=> 6

rtfs> (defn greet [name] (str "Hello, " name "!"))
=> #<function:greet>

rtfs> (greet "World")
=> "Hello, World!"
```
