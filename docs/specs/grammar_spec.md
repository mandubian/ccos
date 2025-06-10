# RTFS - Formal Grammar Specification (EBNF-like Draft)

This document provides a preliminary, EBNF-like grammar for the standalone RTFS language syntax. It aims for clarity rather than strict formal correctness at this stage. Assumes a basic S-expression structure is parsed first.

```ebnf
(* --- Entry Point --- *)
program ::= module_definition | task_definition | agent_profile_definition | expression (* A file typically contains one module, one task, or one agent profile *)

module_definition ::= "(" "module" namespaced_identifier export_option? definition* ")"
export_option ::= "(:exports" "[" identifier+ "]" ")"

definition ::= def_expr | defn_expr | import_definition (* Top-level definitions within a module *)

import_definition ::= "(" "import" namespaced_identifier import_options? ")"
import_options ::= "(:as" identifier ")" | "(:only" "[" identifier+ "]" ")"

defn_expr ::= "(" "defn" symbol "[" param_def* ["&" variable]? "]" [":" type_expr]? expression+ ")" (* Function definition *)

task_definition ::= "(" "task" task_property+ ")"
task_property ::= ":id" string_literal
                 | ":metadata" map_literal
                 | ":intent" expression
                 | ":contracts" contract_definition_map (* Modified from map_literal *)
                 | ":plan" expression
                 | ":execution-trace" vector_literal

contract_definition_map ::= "{" contract_entry* "}"
contract_entry ::= ":provides" map_literal (* Describes capabilities this task offers *)
                 | ":requires" required_capabilities_list (* Describes capabilities this task needs from other agents *)
                 | keyword map_literal (* Allows for other contract aspects, e.g., :sla, :data-handling *)

required_capabilities_list ::= "[" required_capability_entry* "]"
required_capability_entry ::= "{" required_capability_property+ "}"
required_capability_property ::= ":capability-id" string_literal (* ID of the capability, e.g., "vendor.translate-text-v2" *)
                               | ":agent-profile-uri" string_literal (* Optional: URI to a specific agent profile document or discovery hint *)
                               | ":version-constraint" string_literal (* Optional: e.g., \">=1.0 <2.0\" *)
                               | ":optional" boolean (* Default false. If true, task can proceed if capability not found/failed. *)
                               | ":alias" symbol (* Local alias for this capability used in the :plan, e.g., 'translator' *)
                               | ":timeout-ms" integer (* Optional: default timeout for invoking this capability *)
                               | ":retry-policy" map_literal (* Optional: default retry policy *)

(* --- Agent Profile Definition --- *)
agent_profile_definition ::= "(" "agent-profile" agent_profile_property+ ")"
agent_profile_property ::= ":id" string_literal
                         | ":metadata" map_literal
                         | ":capabilities" "[" capability_definition* "]"
                         | ":communication-endpoints" "[" communication_endpoint_definition* "]"
                         | ":discovery-mechanisms" "[" discovery_mechanism_definition* "]"
                         | ":interoperability" map_literal (* e.g., {:a2a-profile-uri "...", :mcp-schema-ref "..."} *)

capability_definition ::= "{" capability_property+ "}"
capability_property ::= ":capability-id" string_literal
                      | ":description" string_literal
                      | ":type" capability_type_keyword
                      | ":input-schema" type_expr
                      | ":output-schema" type_expr
                      | ":annotations" map_literal

capability_type_keyword ::= ":task" | ":tool" | ":service" | ":stream-source" | ":stream-sink" | keyword (* Allow for extension *)

communication_endpoint_definition ::= "{" communication_endpoint_property+ "}"
communication_endpoint_property ::= ":endpoint-id" string_literal
                                  | ":protocol" keyword (* e.g., :json-rpc, :websocket, :http-rest *)
                                  | ":transport" keyword (* e.g., :http, :https, :ws, :wss *)
                                  | ":uri" string_literal
                                  | ":details" map_literal (* Protocol-specific details, including stream-options *)
                                  | ":provides-capabilities" "[" string_literal* "]"

discovery_mechanism_definition ::= "{" discovery_mechanism_property+ "}"
discovery_mechanism_property ::= ":type" keyword (* e.g., :mdns, :registry, :static *)
                               | ":details" map_literal (* Mechanism-specific details *)


(* --- Basic Values --- *)
value ::= literal | variable | list | vector | map | function_call | special_form

literal ::= integer | float | string | boolean | nil | keyword | symbol

integer ::= ["-"] digit+
float   ::= ["-"] digit+ "." digit+ [("e" | "E") ["+" | "-"] digit+]?
string  ::= "\"" (string_char | escape_sequence)* "\""
boolean ::= "true" | "false"
nil     ::= "nil"
keyword ::= SIMPLE_KEYWORD | qualified_keyword ;; Simple: :foo, Qualified: :ns/foo
symbol  ::= identifier (* In code, often implicitly quoted by reader, e.g. 'sym or sym depending on context *)

string_inner ::= /* content of a string literal, used for quoted keywords/symbols */
string_char ::= /* any character except \" or \\ */
escape_sequence ::= "\\" ("\"" | "\\" | "/" | "b" | "f" | "n" | "r" | "t" | unicode_escape)
unicode_escape ::= "u" hex_digit hex_digit hex_digit hex_digit
hex_digit ::= digit | "a".."f" | "A".."F"

variable ::= identifier | namespaced_identifier

identifier ::= identifier_start_char identifier_chars?
namespaced_identifier ::= identifier '/' identifier ;; e.g., my.module/my-function
qualified_keyword ::= ':' identifier '/' identifier ;; e.g., :my.module/my-keyword (Less common, usage TBD)

(* Represents a symbol potentially qualified by a namespace. 
   Namespace parts use '.' separator, symbol uses '/' separator. *)
namespaced_symbol ::= identifier ( ("." identifier)+ "/" identifier | "/" identifier ) 
                    | identifier (* Simple unqualified identifier *)

identifier_start_char ::= letter | "_" | "$" | "+" | "-" | "*" | "/" | "=" | "<" | ">" | "!" | "?" (* Adjust based on desired allowed chars *)
identifier_chars ::= (identifier_start_char | digit | "." | "-")* (* '.' allowed for ns parts, '-' common in symbols *)

(* --- Collections --- *)
list ::= "(" value* ")" (* Represents code forms or literal lists depending on context *)
vector ::= "[" value* "]" (* Typically literal data vector *)
map ::= "{" map_entry* "}"
map_entry ::= map_key value
map_key ::= keyword | string | integer (* Allow other hashable literals? TBD *) 

(* --- Core Forms (Expressions within :plan or function bodies) --- *)
expression ::= literal
             | variable
             | task_context_access
             | list
             | vector
             | map
             | function_call
             | invoke_capability_expr (* Added for invoking external/agent capabilities *)
             | special_form

invoke_capability_expr ::= "(" "invoke" capability_target capability_args_map [invoke_options_map]? ")"
capability_target ::= symbol (* Alias defined in :requires, or a globally resolved capability ID *)
                    | string_literal (* Direct capability ID, if not aliased or needs explicit namespacing *)
capability_args_map ::= map_literal (* Arguments for the capability, must match its :input-schema *)
invoke_options_map ::= "{" invoke_option* "}" (* Override defaults from :requires or agent profile *)
invoke_option ::= ":timeout-ms" integer
                | ":retry-policy" map_literal
                | ":endpoint-override" string_literal (* URI to specific endpoint *)
                | ":auth-override" map_literal (* Authentication details if not handled by agent profile *)
                | keyword literal (* Other invocation-specific options *)

special_form ::= let_expr | if_expr | do_expr | fn_expr | def_expr | parallel_expr | with_resource_expr | try_catch_expr | match_expr | log_step_expr | consume_stream_expr | produce_to_stream_expr (* Added stream forms *)

let_expr ::= "(" "let" "[" let_binding+ "]" expression+ ")"
let_binding ::= binding_pattern [":" type_expr]? expression (* Allow patterns in let *)

if_expr ::= "(" "if" expression expression expression ")"

do_expr ::= "(" "do" expression+ ")"

fn_expr ::= "(" "fn" "[" param_def* ["&" variable]? "]" [":" type_expr]? expression+ ")" (* Added variadic param *)
param_def ::= binding_pattern [":" type_expr]? (* Allow patterns in params *)

def_expr ::= "(" "def" symbol [":" type_expr]? expression ")" (* Variable definition *)

parallel_expr ::= "(" "parallel" parallel_binding+ ")"
parallel_binding ::= "[" variable [":" type_expr]? expression "]"

with_resource_expr ::= "(" "with-resource" "[" variable type_expr expression "]" expression+ ")"

try_catch_expr ::= "(" "try" expression+ catch_clause+ [finally_clause]? ")" (* Added optional finally *)
catch_clause ::= "(" "catch" catch_pattern variable expression+ ")"
finally_clause ::= "(" "finally" expression+ ")"
catch_pattern ::= type_expr | keyword (* e.g., :error/network, :any *) | variable (* Implies catching :any and binding error object *)

match_expr ::= "(" "match" expression match_clause+ ")"
match_clause ::= "(" match_pattern ["when" expression]? expression+ ")" (* Added optional guard *)
match_pattern ::= literal 
                | variable 
                | keyword (* e.g., :ok, :error *) 
                | "_" (* Wildcard *)
                | type_expr (* Match based on type - less common for structural types *)
                | vector_pattern (* Align with vector_destructuring_pattern *)
                | map_pattern (* Align with map_destructuring_pattern *)
                | "(" ":as" variable match_pattern ")" (* Binding pattern *)

vector_pattern ::= "[" match_pattern* ["&" variable]? "]"
map_pattern ::= "{" (map_pattern_entry)* ["&" variable]? "}"
map_pattern_entry ::= map_key match_pattern

(* --- Stream Interaction Forms --- *)
consume_stream_expr ::= "(" "consume-stream" capability_target stream_params_map "{" stream_item_binding "=>" expression+ "}" [stream_options_map]? ")"
stream_item_binding ::= binding_pattern (* Pattern to bind each item from the stream, matching capability's :output-schema item type *)
stream_params_map ::= map_literal (* Parameters to initiate/configure the stream, matching capability's :input-schema *)
stream_options_map ::= "{" stream_option* "}"
stream_option ::= ":on-error" expression (* Expression to evaluate on stream error. Error object bound to a conventional var? *)
                | ":on-complete" expression (* Expression to evaluate on stream completion *)
                | ":buffer-size" integer
                | ":timeout-ms" integer (* Timeout for the whole stream consumption or inactivity *)
                | keyword literal

produce_to_stream_expr ::= "(" "produce-to-stream" capability_target item_expression [stream_produce_options_map]? ")"
item_expression ::= expression (* The item to send, matching capability's :input-schema item type *)
stream_produce_options_map ::= "{" stream_produce_option* "}"
stream_produce_option ::= ":ack-timeout-ms" integer (* Timeout for waiting for acknowledgement if applicable *)
                        | ":on-ack" expression (* Expression to run on successful acknowledgement *)
                        | ":on-nack" expression (* Expression to run on negative acknowledgement/failure *)
                        | keyword literal

(* --- Destructuring Patterns (Used in let, fn, match) --- *)

(* Note on Patterns: `binding_pattern` is used for assigning values in `let` and `fn` parameters. 
   `match_pattern` is used within `match` clauses. While they share significant structure 
   (especially for maps and vectors), `match_pattern` includes additional forms like literals, 
   wildcards (`_`), and potentially type matching, which are not typically used for simple binding. 
   The grammar aims to keep the common structures (map/vector destructuring) consistent between them. *)

binding_pattern ::= variable | map_destructuring_pattern | vector_destructuring_pattern

map_destructuring_pattern ::= "{" map_destructuring_entry*
                              ["&" variable]? ;; Optional binding for rest of map
                              [":as" variable]? ;; Optional binding for the whole map
                              "}"
map_destructuring_entry ::= map_destructuring_key_entry | map_destructuring_or_entry
map_destructuring_key_entry ::= ":keys" "[" binding_pattern+ "]" ;; Bind values by keyword keys
                              | keyword binding_pattern ;; Bind specific key to pattern/var
                              | string binding_pattern ;; Bind specific string key
map_destructuring_or_entry ::= ":or" "{" (variable literal)+ "}" ;; Default values

vector_destructuring_pattern ::= "[" binding_pattern* ["&" variable]? [":as" variable]? "]"

(* --- Types --- *)

type_expr ::= primitive_type
            | collection_type
            | stream_type (* Added stream_type here *)
            | function_type
            | resource_type
            | union_type
            | intersection_type
            | literal_type (* e.g., [:val 42] *)
            | type_variable (* For generics, if added later *)

primitive_type ::= ":int" | ":float" | ":string" | ":bool" | ":nil" | ":keyword" | ":symbol" | ":any" | ":never"

collection_type ::= vector_type | list_type | tuple_type | map_type | array_type ;; Added array_type

stream_type ::= "[:stream" type_expr "]" (* Definition for stream type *)

vector_type ::= "[:vector" type_expr [shape_1d]? "]" ;; Optionally allow specifying size
list_type ::= "[:list" type_expr "]" (* If distinct from vector *)
tuple_type ::= "[:tuple" type_expr+ "]"
map_type ::= "[:map" map_type_entry* map_type_wildcard? "]"
map_type_entry ::= "[" keyword type_expr [\\\"?\\"]? "]" (* Optional marker '?'. Enforce keyword keys. *)
map_type_wildcard ::= "[:*" type_expr "]" (* Allows additional keys of this type *)
array_type ::= "[" ":array" type_expr shape? "]" ;; Multi-dimensional array/tensor type

shape ::= "[" dimension* "]" ;; Shape specification (e.g., [100 100 3])
shape_1d ::= "[" dimension "]" ;; Shape for 1D vector
dimension ::= integer | "?" ;; Dimension size (integer) or unknown/dynamic ("?")

function_type ::= "[:=>" fn_param_list fn_return_type "]"
fn_param_list ::= "[" type_expr* [fn_variadic_param]? "]" (* Zero or more fixed, optional variadic at end *)
fn_variadic_param ::= "[:*" type_expr "]" (* Represents & rest *)
fn_return_type ::= type_expr

resource_type ::= "[:resource" symbol "]" (* Explicit resource type syntax *)

union_type ::= "[:or" type_expr+ "]"
intersection_type ::= "[:and" type_expr predicate_expr+ "]"
predicate_expr ::= "[" predicate_name literal* "]"
predicate_name ::= keyword | symbol (* e.g., :>, :string-starts-with *)

literal_type ::= "[:val" literal "]"

type_variable ::= symbol (* Placeholder for potential future generics *)

(* --- Comments --- *)
comment ::= ";" /* any character until end of line */

(* Base definitions like letter, digit assumed *) 
letter ::= "a".."z" | "A".."Z"
digit ::= "0".."9"
```

**Refinements Made:**

*   **Identifiers:** Defined `identifier_start_char` and `identifier_chars` more explicitly, allowing common Lisp/Clojure characters. Changed `+` to `*` in `identifier_chars` to allow single-character identifiers. Added `.` and `-` to `identifier_chars`.
*   **Keywords:** Clarified that keywords can be formed from identifiers or quoted strings (`:"string key"`). Added `string_inner` definition.
*   **Strings:** Added basic `escape_sequence` definition including `\uXXXX`.
*   **Map Keys:** Explicitly allowed `keyword`, `string`, `integer` as map keys, marked as TBD for other literals.
*   **Variable:** Updated `variable` rule to include `task_context_access` and `namespaced_symbol`.
*   **Namespaced Symbols:** Refined `namespaced_symbol` rule to better reflect `ns.part.part/symbol` structure. Adjusted `identifier_chars` slightly.
*   **Function Definitions (`fn`, `defn`):** Added optional `& rest` parameter for variadic functions. Added `defn` form.
*   **`try/catch`:** Added optional `finally` clause.
*   **`match`:** Added optional `when` guard expression to clauses. Added wildcard `_` and `:as` binding patterns.
*   **Task Context:** Added `task_context_access` rule for `@intent` etc.
*   **Types:** Refactored `type_expr` slightly for clarity. Defined `fn_param_list` for function types. Added `[:* Type]` for open map schemas. Added `array_type` and `shape`.
*   **Modules:** Added `module_definition`, `module_name`, `module_option`, `exports_option`, `module_form`, `import_form`, `import_option`, `as_option`, `only_option`. Updated `program` entry point. Updated `variable` and `expression` to include module-related forms. Changed `def` and `defn` to use `symbol` instead of `variable` as the thing being defined.
*   **Comments:** Added basic single-line comment rule.

**Further Refinements Needed:**

*   Precise character sets for identifiers.
*   Handling of reader macros (like `'` for quote, `@` for deref/task-context).
*   More detailed map/vector pattern matching syntax (destructuring).
*   Formal definition of whitespace handling.
*   Resolution of ambiguities (e.g., `(a b)` as list literal vs function call).
*   Placement rules for `import_form` (e.g., only at top level of module/plan?).
