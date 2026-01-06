# RTFS Teaching MCP Tools for `ccos-mcp.rs`

**Author:** Auto-generated  
**Date:** 2026-01-06  
**Status:** Draft

---

## Overview

Expose RTFS language learning facilities as MCP tools so AI agents can learn RTFS syntax, compile code, understand errors, and get repair suggestions.

### Rationale

RTFS is a Lisp-like language for autonomous AI agents, but LLMs aren't trained on it. By exposing grammar, samples, compiler, and error explainer as MCP tools, we create a feedback loop:

1. **Learn syntax** → grammar + samples tools
2. **Practice writing** → compiler validation
3. **Learn from mistakes** → error explanation + repair

---

## Tools to Implement

All tools are registered in `register_ccos_tools()` in `ccos/src/bin/ccos-mcp.rs`.

---

### 1. `rtfs_get_grammar`

**Purpose:** Return RTFS grammar reference by category.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "category": {
      "type": "string",
      "enum": ["overview", "literals", "collections", "special_forms", "types", "all"],
      "default": "overview",
      "description": "Grammar category to retrieve"
    }
  }
}
```

**Output:** Markdown-formatted grammar documentation.

**Implementation:**

```rust
server.register_tool(
    "rtfs_get_grammar",
    "Get RTFS language grammar reference. Returns syntax rules by category.",
    json!({
        "type": "object",
        "properties": {
            "category": {
                "type": "string",
                "enum": ["overview", "literals", "collections", "special_forms", "types", "all"],
                "default": "overview"
            }
        }
    }),
    Box::new(move |params| {
        Box::pin(async move {
            let category = params.get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("overview");
            
            let content = match category {
                "overview" => GRAMMAR_OVERVIEW,
                "literals" => GRAMMAR_LITERALS,
                "collections" => GRAMMAR_COLLECTIONS,
                "special_forms" => GRAMMAR_SPECIAL_FORMS,
                "types" => GRAMMAR_TYPES,
                "all" => &format!("{}\n\n{}\n\n{}\n\n{}\n\n{}",
                    GRAMMAR_OVERVIEW, GRAMMAR_LITERALS, GRAMMAR_COLLECTIONS,
                    GRAMMAR_SPECIAL_FORMS, GRAMMAR_TYPES),
                _ => GRAMMAR_OVERVIEW,
            };
            
            Ok(json!({
                "category": category,
                "content": content
            }))
        })
    }),
);
```

**Grammar Constants (embed as static strings):**

```rust
const GRAMMAR_OVERVIEW: &str = r#"
# RTFS Grammar Overview

RTFS uses **homoiconic s-expression syntax** where code and data share the same representation.

## Core Syntax
- **Lists:** `(func arg1 arg2)` - Code and function calls
- **Vectors:** `[1 2 3]` - Ordered sequences  
- **Maps:** `{:key "value"}` - Key-value associations
- **Comments:** `;; single line` or `#| block |#`

## Basic Pattern
```clojure
(operator operand1 operand2 ...)
```
"#;

const GRAMMAR_LITERALS: &str = r#"
# RTFS Literals

## Primitives
```clojure
42                    ; integer
3.14                  ; float
"hello world"         ; string
true / false          ; boolean
nil                   ; null value
```

## Extended Types
```clojure
:keyword              ; keyword (starts with :)
:my.ns/qualified      ; qualified keyword
2026-01-06T10:00:00Z  ; timestamp
abc123-def456...      ; UUID format
resource://handle     ; resource handle
```
"#;

const GRAMMAR_COLLECTIONS: &str = r#"
# RTFS Collections

## Lists (Code)
```clojure
(+ 1 2 3)              ; function call
(if true "yes" "no")   ; special form
```

## Vectors (Data)
```clojure
[1 2 3 4]              ; literal vector
(vector 1 2 3)         ; construct vector
```

## Maps
```clojure
{:name "Alice" :age 30}        ; map literal
{:key1 "value1" :key2 42}      ; mixed values
```
"#;

const GRAMMAR_SPECIAL_FORMS: &str = r#"
# RTFS Special Forms

## Variable Binding
```clojure
;; let - lexical scoping
(let [x 1
      y (+ x 2)]
  (* x y))

;; def - global definition
(def pi 3.14159)

;; defn - function definition
(defn add [x y]
  (+ x y))
```

## Control Flow
```clojure
;; if - conditional
(if (> x 0)
  "positive"
  "non-positive")

;; do - sequencing
(do
  (println "first")
  (println "second")
  42)

;; match - pattern matching
(match value
  0 "zero"
  n (str "number: " n)
  _ "other")
```

## Functions
```clojure
;; anonymous function
(fn [x] (* x x))

;; variadic function
(defn sum [& args]
  (reduce + 0 args))
```

## Host Integration
```clojure
;; call CCOS capability
(call :ccos.state.kv/get "my-key")
(call "ccos.http.request" {:url "..."})
```
"#;

const GRAMMAR_TYPES: &str = r#"
# RTFS Type Expressions

## Primitive Types
```clojure
:int :float :string :bool :nil
```

## Collection Types
```clojure
[:vector :int]              ; vector of integers
[:tuple :string :int]       ; tuple type
[:map [:name :string] [:age :int]]  ; map schema
```

## Function Types
```clojure
[:fn [:int :int] :int]      ; (int, int) -> int
```

## Union & Optional
```clojure
[:union :int :string]       ; either int or string
:string?                    ; optional string (= [:union :string :nil])
```

## Refined Types
```clojure
[:and :int [:> 0] [:< 100]] ; int between 1 and 99
[:and :string [:min-length 1] [:max-length 255]]
```
"#;
```

---

### 2. `rtfs_get_samples`

**Purpose:** Return curated RTFS code examples by category.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "category": {
      "type": "string",
      "enum": ["basic", "bindings", "control_flow", "functions", "capabilities", "types", "all"],
      "default": "basic"
    }
  }
}
```

**Implementation:**

```rust
server.register_tool(
    "rtfs_get_samples",
    "Get example RTFS code snippets by category",
    json!({
        "type": "object",
        "properties": {
            "category": {
                "type": "string",
                "enum": ["basic", "bindings", "control_flow", "functions", "capabilities", "types", "all"],
                "default": "basic"
            }
        }
    }),
    Box::new(move |params| {
        Box::pin(async move {
            let category = params.get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("basic");
            
            let samples = get_samples_for_category(category);
            
            Ok(json!({
                "category": category,
                "samples": samples
            }))
        })
    }),
);

fn get_samples_for_category(category: &str) -> Vec<serde_json::Value> {
    match category {
        "basic" => vec![
            json!({
                "name": "Arithmetic",
                "code": "(+ 1 2 3)",
                "result": "6",
                "explanation": "Add numbers together"
            }),
            json!({
                "name": "String concatenation",
                "code": "(str \"Hello, \" \"World!\")",
                "result": "\"Hello, World!\"",
                "explanation": "Concatenate strings"
            }),
            json!({
                "name": "Vector creation",
                "code": "[1 2 3 4 5]",
                "result": "[1, 2, 3, 4, 5]",
                "explanation": "Create a vector literal"
            }),
        ],
        "bindings" => vec![
            json!({
                "name": "Simple let binding",
                "code": "(let [x 5] x)",
                "result": "5",
                "explanation": "Bind x to 5, return x"
            }),
            json!({
                "name": "Multiple bindings",
                "code": "(let [x 5 y 10] (+ x y))",
                "result": "15",
                "explanation": "Bind multiple values, use in expression"
            }),
            json!({
                "name": "Destructuring",
                "code": "(let [{:keys [name age]} {:name \"Alice\" :age 30}] name)",
                "result": "\"Alice\"",
                "explanation": "Extract values from map using destructuring"
            }),
        ],
        "control_flow" => vec![
            json!({
                "name": "If expression",
                "code": "(if (> 5 3) \"yes\" \"no\")",
                "result": "\"yes\"",
                "explanation": "Conditional expression"
            }),
            json!({
                "name": "Pattern matching",
                "code": "(match 42\n  0 \"zero\"\n  n (str \"number: \" n))",
                "result": "\"number: 42\"",
                "explanation": "Match value against patterns"
            }),
        ],
        "functions" => vec![
            json!({
                "name": "Anonymous function",
                "code": "((fn [x] (* x x)) 5)",
                "result": "25",
                "explanation": "Define and immediately call anonymous function"
            }),
            json!({
                "name": "Named function",
                "code": "(defn greet [name]\n  (str \"Hello, \" name))\n(greet \"World\")",
                "result": "\"Hello, World\"",
                "explanation": "Define named function, then call it"
            }),
            json!({
                "name": "Higher-order function",
                "code": "(map (fn [x] (* x 2)) [1 2 3])",
                "result": "[2, 4, 6]",
                "explanation": "Apply function to each element"
            }),
        ],
        "capabilities" => vec![
            json!({
                "name": "Call capability",
                "code": "(call :ccos.http/get {:url \"https://api.example.com\"})",
                "explanation": "Call CCOS HTTP capability"
            }),
            json!({
                "name": "Capability definition",
                "code": "(capability \"my-tool/greet\"\n  :description \"Greet a user\"\n  :input-schema [:map [:name :string]]\n  :output-schema :string\n  :implementation\n  (fn [input]\n    (str \"Hello, \" (:name input))))",
                "explanation": "Define a new capability with schema"
            }),
        ],
        "types" => vec![
            json!({
                "name": "Type annotation",
                "code": "(defn add [x :int y :int] :int\n  (+ x y))",
                "explanation": "Function with type annotations"
            }),
            json!({
                "name": "Complex schema",
                "code": "[:map\n  [:name :string]\n  [:age [:and :int [:>= 0]]]\n  [:email {:optional true} :string]]",
                "explanation": "Map schema with optional field and refined type"
            }),
        ],
        "all" | _ => {
            // Combine all categories
            let mut all = vec![];
            for cat in ["basic", "bindings", "control_flow", "functions", "capabilities", "types"] {
                all.extend(get_samples_for_category(cat));
            }
            all
        }
    }
}
```

---

### 3. `rtfs_compile`

**Purpose:** Parse/compile RTFS code and return result or detailed errors.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "description": "RTFS source code to compile"
    },
    "show_ast": {
      "type": "boolean",
      "default": false,
      "description": "Include AST representation in output"
    }
  },
  "required": ["code"]
}
```

**Implementation:**

```rust
server.register_tool(
    "rtfs_compile",
    "Compile RTFS code. Returns success info or detailed parse errors.",
    json!({
        "type": "object",
        "properties": {
            "code": { "type": "string", "description": "RTFS source code" },
            "show_ast": { "type": "boolean", "default": false }
        },
        "required": ["code"]
    }),
    Box::new(move |params| {
        Box::pin(async move {
            let code = params.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let show_ast = params.get("show_ast").and_then(|v| v.as_bool()).unwrap_or(false);
            
            if code.is_empty() {
                return Ok(json!({
                    "success": false,
                    "error": "No code provided"
                }));
            }
            
            // Try to parse with enhanced error reporting
            match rtfs::parser::parse_with_enhanced_errors(code, None) {
                Ok(ast) => {
                    let mut result = json!({
                        "success": true,
                        "message": "Compilation successful",
                        "expression_count": ast.len()
                    });
                    
                    if show_ast {
                        result["ast"] = json!(format!("{:#?}", ast));
                    }
                    
                    Ok(result)
                }
                Err(parse_error) => {
                    Ok(json!({
                        "success": false,
                        "error": {
                            "message": parse_error.message,
                            "location": {
                                "line": parse_error.span.as_ref().map(|s| s.start_line),
                                "column": parse_error.span.as_ref().map(|s| s.start_col)
                            },
                            "context": parse_error.context_snippet,
                            "hint": parse_error.hint
                        },
                        "code_preview": code.chars().take(100).collect::<String>()
                    }))
                }
            }
        })
    }),
);
```

---

### 4. `rtfs_explain_error`

**Purpose:** Explain error messages in plain English with common causes and fixes.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "error_message": {
      "type": "string",
      "description": "The error message to explain"
    },
    "code": {
      "type": "string",
      "description": "The code that produced the error (optional)"
    }
  },
  "required": ["error_message"]
}
```

**Implementation:**

```rust
server.register_tool(
    "rtfs_explain_error",
    "Explain an RTFS error message in plain English with common causes",
    json!({
        "type": "object",
        "properties": {
            "error_message": { "type": "string" },
            "code": { "type": "string" }
        },
        "required": ["error_message"]
    }),
    Box::new(move |params| {
        Box::pin(async move {
            let error_msg = params.get("error_message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let code = params.get("code").and_then(|v| v.as_str());
            
            let explanation = explain_rtfs_error(error_msg, code);
            
            Ok(json!(explanation))
        })
    }),
);

fn explain_rtfs_error(error: &str, code: Option<&str>) -> serde_json::Value {
    let error_lower = error.to_lowercase();
    
    // Pattern match common errors
    let (explanation, common_causes, fix_suggestions) = if error_lower.contains("expected") && error_lower.contains("expression") {
        (
            "The parser expected an expression but found something else.",
            vec![
                "Unbalanced parentheses - missing opening or closing paren",
                "Empty list without proper content",
                "Incomplete expression at end of input"
            ],
            vec![
                "Check that all ( have matching )",
                "Ensure lists have at least an operator: (+ 1 2) not ()",
                "Complete any unfinished expressions"
            ]
        )
    } else if error_lower.contains("unexpected") && error_lower.contains("token") {
        (
            "The parser encountered a token it didn't expect in this position.",
            vec![
                "Wrong syntax for the construct being used",
                "Missing required elements",
                "Extra tokens that don't belong"
            ],
            vec![
                "Review the syntax for the form you're using",
                "Check for typos in keywords",
                "Ensure proper ordering of elements"
            ]
        )
    } else if error_lower.contains("unbalanced") || error_lower.contains("unclosed") {
        (
            "There are mismatched brackets in the code.",
            vec![
                "Missing closing ) ] or }",
                "Extra opening ( [ or {",
                "Brackets of wrong type used"
            ],
            vec![
                "Count opening and closing brackets",
                "Use an editor with bracket matching",
                "Remember: () for calls, [] for vectors/bindings, {} for maps"
            ]
        )
    } else if error_lower.contains("undefined") || error_lower.contains("not found") {
        (
            "A symbol or function was referenced but not defined.",
            vec![
                "Typo in function or variable name",
                "Using a variable before it's defined",
                "Missing import or require"
            ],
            vec![
                "Check spelling of the symbol",
                "Ensure definitions come before usage",
                "Use :keys in let bindings for map destructuring"
            ]
        )
    } else {
        (
            "This error indicates a problem with the RTFS code.",
            vec!["Syntax error", "Semantic error"],
            vec![
                "Review the code structure",
                "Compare with working examples",
                "Use rtfs_get_samples to see correct syntax"
            ]
        )
    };
    
    let mut result = json!({
        "error": error,
        "explanation": explanation,
        "common_causes": common_causes,
        "suggestions": fix_suggestions
    });
    
    if let Some(code) = code {
        // Add bracket analysis if code is provided
        let open_parens = code.matches('(').count();
        let close_parens = code.matches(')').count();
        let open_brackets = code.matches('[').count();
        let close_brackets = code.matches(']').count();
        let open_braces = code.matches('{').count();
        let close_braces = code.matches('}').count();
        
        let mut issues = vec![];
        if open_parens != close_parens {
            issues.push(format!("Paren mismatch: {} '(' vs {} ')'", open_parens, close_parens));
        }
        if open_brackets != close_brackets {
            issues.push(format!("Bracket mismatch: {} '[' vs {} ']'", open_brackets, close_brackets));
        }
        if open_braces != close_braces {
            issues.push(format!("Brace mismatch: {} '{{' vs {} '}}'", open_braces, close_braces));
        }
        
        if !issues.is_empty() {
            result["bracket_analysis"] = json!(issues);
        }
    }
    
    result
}
```

---

### 5. `rtfs_repair`

**Purpose:** Suggest repairs for broken RTFS code using heuristics.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "description": "Broken RTFS code"
    },
    "error_message": {
      "type": "string",
      "description": "Optional error message from compilation"
    }
  },
  "required": ["code"]
}
```

**Implementation:**

```rust
server.register_tool(
    "rtfs_repair",
    "Suggest repairs for broken RTFS code",
    json!({
        "type": "object",
        "properties": {
            "code": { "type": "string" },
            "error_message": { "type": "string" }
        },
        "required": ["code"]
    }),
    Box::new(move |params| {
        Box::pin(async move {
            let code = params.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let error = params.get("error_message").and_then(|v| v.as_str());
            
            let suggestions = repair_rtfs_code(code, error);
            
            Ok(json!(suggestions))
        })
    }),
);

fn repair_rtfs_code(code: &str, _error: Option<&str>) -> serde_json::Value {
    let mut suggestions: Vec<serde_json::Value> = vec![];
    
    // 1. Bracket balancing
    let open_parens = code.matches('(').count();
    let close_parens = code.matches(')').count();
    
    if open_parens > close_parens {
        let missing = open_parens - close_parens;
        let repaired = format!("{}{}", code, ")".repeat(missing));
        suggestions.push(json!({
            "type": "bracket_fix",
            "description": format!("Add {} missing closing parenthesis", missing),
            "repaired_code": repaired,
            "confidence": "high"
        }));
    } else if close_parens > open_parens {
        let extra = close_parens - open_parens;
        suggestions.push(json!({
            "type": "bracket_fix",
            "description": format!("Remove {} extra closing parenthesis or add opening", extra),
            "confidence": "medium"
        }));
    }
    
    // 2. Common typos
    let typo_fixes = vec![
        ("defun", "defn"),
        ("lambda", "fn"),
        ("define", "def"),
        ("cond", "match"),
        ("null", "nil"),
        ("True", "true"),
        ("False", "false"),
        ("None", "nil"),
    ];
    
    for (wrong, right) in typo_fixes {
        if code.contains(wrong) {
            suggestions.push(json!({
                "type": "typo_fix",
                "description": format!("Replace '{}' with '{}'", wrong, right),
                "repaired_code": code.replace(wrong, right),
                "confidence": "high"
            }));
        }
    }
    
    // 3. Missing let bindings format
    if code.contains("let ") && !code.contains("let [") {
        suggestions.push(json!({
            "type": "syntax_fix",
            "description": "let requires bindings in square brackets: (let [x 1] ...)",
            "example": "(let [x 1 y 2] (+ x y))",
            "confidence": "medium"
        }));
    }
    
    // 4. If code still doesn't parse after fixes
    let best_repair = if let Some(first) = suggestions.first() {
        first.get("repaired_code").and_then(|v| v.as_str()).map(String::from)
    } else {
        None
    };
    
    // Verify if repair works
    let repair_valid = if let Some(ref repair) = best_repair {
        rtfs::parser::parse(repair).is_ok()
    } else {
        false
    };
    
    json!({
        "original_code": code,
        "suggestions": suggestions,
        "best_repair": best_repair,
        "repair_valid": repair_valid,
        "note": "Suggestions are heuristic-based. Review before using."
    })
}
```

---

## File Location

All implementations go in:
- **File:** `ccos/src/bin/ccos-mcp.rs`
- **Function:** `register_ccos_tools()`
- **Position:** After existing tools (around line 1896)

Add the grammar/sample constants at the top of the file or in a separate module:
- Option A: Inline `const` strings in `ccos-mcp.rs`
- Option B: New module `ccos/src/mcp/rtfs_teaching.rs` with re-export

---

## Dependencies

No new dependencies required. Uses existing:
- `rtfs::parser::parse()`
- `rtfs::parser::parse_with_enhanced_errors()`
- `serde_json::json!`

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod rtfs_teaching_tests {
    use super::*;
    
    #[test]
    fn test_grammar_overview() {
        assert!(GRAMMAR_OVERVIEW.contains("s-expression"));
    }
    
    #[test]
    fn test_compile_valid_code() {
        let result = rtfs::parser::parse("(+ 1 2)");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_compile_invalid_code() {
        let result = rtfs::parser::parse("(+ 1 2");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_error_explanation() {
        let explanation = explain_rtfs_error("expected expression", None);
        assert!(explanation["explanation"].as_str().unwrap().contains("expected"));
    }
    
    #[test]
    fn test_repair_missing_paren() {
        let repair = repair_rtfs_code("(+ 1 2", None);
        assert_eq!(repair["best_repair"].as_str(), Some("(+ 1 2)"));
    }
}
```

### Manual Testing

```bash
# Build
cargo build --bin ccos-mcp

# Run with stdio transport
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rtfs_get_grammar","arguments":{"category":"overview"}}}' | ./target/debug/ccos-mcp --transport stdio

# Test compilation
echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"rtfs_compile","arguments":{"code":"(+ 1 2 3)"}}}' | ./target/debug/ccos-mcp --transport stdio
```

---

## Future Enhancements

1. **LLM-assisted repair:** Use discovery service for complex repairs
2. **Interactive tutorial:** Step-by-step learning sequences
3. **Type checking:** Validate types against schemas
4. **Execution sandbox:** Run code in safe microVM

---

## References

- Grammar: `rtfs/src/rtfs.pest`
- Syntax spec: `docs/rtfs-2.0/specs/02-syntax-and-grammar.md`
- Parser: `rtfs/src/parser/mod.rs`
- Sample capability: `capabilities/core/meta-planner.rtfs`
- Development tooling: `rtfs/src/development_tooling.rs`
