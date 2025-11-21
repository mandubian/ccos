# RTFS 2.0 Macro System

## Overview

**Note**: The macro system described in this document is a **design target** and is **not currently implemented** in RTFS 2.0. This specification represents the planned macro system for future versions.

RTFS is designed to support a simple macro system for code transformation using quasiquote and unquote, but this functionality is not yet available in the current implementation.

## Planned Macro Definition

Macros would be defined using `defmacro` (not yet implemented):

```rtfs
;; Planned macro syntax (not yet implemented)
(defmacro when [condition body]
  `(if ~condition ~body))

;; Planned usage (not yet implemented)
(when (> x 0)
  (println "positive"))
```

**Current Workaround**: Use explicit control flow constructs like `if` and `do` instead of macros.

## Quasiquote and Unquote

### Quasiquote (`)
Creates code templates:

```rtfs
;; Quasiquote creates a code template
`(list 1 2 3)  ; => (list 1 2 3) as code

;; Unquote (~) inserts values
(let [x 42]
  `(println ~x))  ; => (println 42)
```

## Macro Expansion

Macros expand at compile time:

```rtfs
;; Before expansion
(when (> x 0) (println "ok"))

;; After expansion
(if (> x 0) (println "ok"))
```

## Common Patterns

### Control Structures

```rtfs
;; unless - opposite of if
(defmacro unless [condition body]
  `(if (not ~condition) ~body))

;; Simple conditional
(defmacro cond [clauses]
  (if (empty? clauses)
    nil
    (let [[test then & rest] clauses]
      (if (= test :else)
        then
        `(if ~test ~then (cond ~rest))))))
```

This simple macro system enables basic code transformation while maintaining RTFS's functional nature.</content>
<parameter name="filePath">/home/mandubian/workspaces/mandubian/ccos/docs/rtfs-2.0/specs-new/05-macro-system.md