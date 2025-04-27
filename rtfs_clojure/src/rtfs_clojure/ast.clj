(ns rtfs-clojure.ast
  "Defines the Abstract Syntax Tree (AST) nodes for RTFS (Reasoning Task Flow Specification),
   centered around the 'task' artifact.")

;;--------------------------------------------------------------------------
;; Core Expression Nodes (used within :plan)
;;--------------------------------------------------------------------------

;; Represents literal values (numbers, strings, booleans, nil, keywords)
(defrecord Literal [value])

;; Represents symbols (variables, function names)
(defrecord Symbol [name]) ; Name stored as string, consistent with previous decision

;; Represents a list structure, typically a function/tool call or special form
;; Example: (+ 1 2), (tool:read-file "path"), (if c a b)
;; For now, a generic list representation. Specific forms might get dedicated records later.
(defrecord Call [elements]) ; elements is a sequence of other AST nodes

;; Represents sequential execution: (do expr1 expr2 ...)
(defrecord Do [body]) ; body is a sequence of AST nodes

;; Represents parallel execution: (parallel [id1 expr1] [id2 expr2] ...)
(defrecord ParallelBinding [id expr]) ; id is a Symbol node, expr is an AST node
(defrecord Parallel [bindings]) ; bindings is a sequence of ParallelBinding records

;; Represents joining parallel steps: (join id1 id2 ...)
(defrecord Join [ids]) ; ids is a sequence of Symbol nodes

;; Represents logging a step: (log-step :id <id> <expr>)
(defrecord LogStep [id expr]) ; id is a Symbol node, expr is an AST node

;; Represents a definition: (def symbol value)
(defrecord Def [symbol value]) ; symbol is a Symbol node, value is an AST node

;; Represents a single binding in a let form: [symbol expr]
(defrecord LetBinding [symbol expr]) ; symbol is a Symbol node, expr is an AST node

;; Represents local bindings: (let [bindings] body...)
(defrecord Let [bindings body]) ; bindings is a sequence of LetBinding, body is a sequence of AST nodes

;; Represents conditional execution: (if condition then else)
(defrecord If [condition then-branch else-branch]) ; All are AST nodes

;; Represents an anonymous function: (fn [params] body...)
(defrecord Fn [params body]) ; params is a sequence of Symbol nodes, body is a sequence of AST nodes

;;--------------------------------------------------------------------------
;; Execution Log Nodes (used within :execution-log)
;;--------------------------------------------------------------------------

;; Represents a single entry in the execution log
(defrecord LogEntry [stage agent timestamp status derived-from plan result error executing-step executed-step])
;; Note: Not all fields may be present in every entry. `plan` might be the AST,
;; `result`/`error` would be AST Literal nodes or specific error structures.
;; `executing-step`/`executed-step` would be Symbol nodes referencing step IDs.

;;--------------------------------------------------------------------------
;; Task Node (The Central Artifact)
;;--------------------------------------------------------------------------

;; Represents the main Task structure
(defrecord Task [id source natural-language intent plan execution-log])
;; - id: String or Symbol identifier for the task
;; - source: String indicating origin (e.g., "human-instruction", "system-generated")
;; - natural-language: String containing the original request (optional)
;; - intent: Map representing the structured semantic goal
;; - plan: Sequence of core expression AST nodes representing the execution steps
;; - execution-log: Vector of LogEntry records

;;--------------------------------------------------------------------------
;; Helper Functions
;;--------------------------------------------------------------------------

(defn make-literal [value] (->Literal value))
(defn make-symbol [input] (->Symbol (name input))) ; Use name instead of str
(defn make-call [elements] (->Call (vec elements))) ; Store elements as vector
(defn make-do [body] (->Do (vec body)))
(defn make-parallel-binding [id-node expr-node] (->ParallelBinding id-node expr-node))
(defn make-parallel [bindings] (->Parallel (vec bindings)))
(defn make-join [id-nodes] (->Join (vec id-nodes)))
(defn make-log-step [id-node expr-node] (->LogStep id-node expr-node))

(defn make-def [symbol-node value-node] (->Def symbol-node value-node))
(defn make-let-binding [symbol-node expr-node] (->LetBinding symbol-node expr-node))
(defn make-let [binding-nodes body-nodes] (->Let (vec binding-nodes) (vec body-nodes)))
(defn make-if [condition-node then-node else-node] (->If condition-node then-node else-node))
(defn make-fn [param-nodes body-nodes] (->Fn (vec param-nodes) (vec body-nodes)))

(defn make-log-entry [& {:as fields}]
  (map->LogEntry fields))

(defn make-task [& {:keys [id source natural-language intent plan execution-log]
                    :or {intent {} plan [] execution-log []}}]
  (->Task id source natural-language intent (vec plan) (vec execution-log)))
