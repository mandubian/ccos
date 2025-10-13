# RTFS 2.0 Macro System

## Overview

RTFS provides a simple macro system for code transformation using quasiquote and unquote.

## Macro Definition

Macros are defined using `defmacro`:

```rtfs
;; Simple macro
(defmacro when [condition body]
  `(if ~condition ~body))

;; Usage
(when (> x 0)
  (println "positive"))
```

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