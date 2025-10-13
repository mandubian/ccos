# RTFS 2.0: Grammar and Syntax

## 1. Core Syntax: The S-Expression

The fundamental building block of RTFS is the **s-expression** (symbolic expression). An s-expression is either an **atom** or a **list** of other s-expressions. This simple, recursive structure is used to represent both code and data.

### Atoms

Atoms are the indivisible elements of the language. RTFS supports the following atomic types:

-   **Integer**: A whole number, e.g., `42`, `-100`.
-   **String**: A sequence of characters enclosed in double quotes, e.g., `"hello, world"`, `"\"escaped\""`.
-   **Symbol**: An identifier used to name variables and functions, e.g., `x`, `+`, `my-function`. Symbols can contain letters, numbers, and most punctuation characters, but cannot start with a number.
-   **Keyword**: A special type of symbol that starts with a colon (`:`). Keywords evaluate to themselves and are often used as keys in maps or as identifiers for capabilities, e.g., `:key`, `:fs.read`.
-   **Boolean**: Represents truth values, either `true` or `false`.
-   **Nil**: Represents the absence of a value, written as `nil`.

### Lists

A list is a sequence of s-expressions enclosed in parentheses `()`. Lists are the primary mechanism for grouping elements and representing function calls.

```rtfs
;; A list of numbers
(1 2 3)

;; A list containing different atom types
("hello" 42 my-symbol :a-keyword)

;; A nested list, representing a tree structure
(a (b c) (d (e)))
```

A list is also the syntax for a **function call**. The first element of the list is the function to be called, and the remaining elements are the arguments.

```rtfs
;; A call to the '+' function with arguments 1 and 2
(+ 1 2)
```

## 2. Program Structure

An RTFS program is simply a sequence of s-expressions. These expressions are typically read and evaluated in order from a source file.

```rtfs
;; file: my-program.rtfs

;; A function definition
(def my-square (fn (x) (* x x)))

;; A call to the function
(my-square 10)
```

The result of the program is the result of the last evaluated expression.

## 3. Comments

Comments are ignored by the evaluator. RTFS supports single-line comments starting with a semicolon `;`. Everything from the semicolon to the end of the line is a comment.

```rtfs
;; This is a full-line comment.
(+ 1 2) ;; This is a comment on the same line as code.
```

There is no block comment syntax.

## 4. Collections: Lists, Vectors, and Maps

In addition to the fundamental list structure, RTFS provides syntax for two other common collection types.

### Vectors

A vector is an ordered, indexed collection, similar to a list but often with different performance characteristics for random access. Vectors are denoted by square brackets `[]`.

```rtfs
;; A vector of numbers
[1 2 3]

;; A vector can be accessed by index (using a host capability or stdlib function)
;; (get [10 20 30] 1)  => 20
```

### Maps

A map is a collection of key-value pairs. Maps are denoted by curly braces `{}`. Keys and values can be any RTFS data type.

```rtfs
;; A map with keyword keys
{:name "Alice" :age 30}

;; A map with string keys
{"first-name" "Bob" "last-name" "Smith"}

;; A nested map
{:user {:id 123 :roles ["admin" "editor"]}}
```

## 5. Whitespace

Whitespace (spaces, tabs, newlines) is used to separate atoms and lists but is otherwise insignificant. The following forms are equivalent:

```rtfs
(+ 1 2)
```

```rtfs
(
  +
  1
  2
)
```

This flexibility allows for code to be formatted for readability.

## 6. Summary of Syntax

| Construct     | Syntax                                      | Example                               |
|---------------|---------------------------------------------|---------------------------------------|
| **Atom**      |                                             | `42`, `"hi"`, `my-var`, `:key`, `true`  |
| **List**      | `(elem1 elem2 ...)`                         | `(+ 1 2)`                             |
| **Vector**    | `[elem1 elem2 ...]`                         | `[10 20 30]`                          |
| **Map**       | `{key1 val1 key2 val2 ...}`                 | `{:a 1 :b 2}`                         |
| **Comment**   | `; text to end of line`                     | `;; a comment`                        |
| **Function Call** | `(function-name arg1 arg2 ...)`         | `(str "a" "b")`                       |
