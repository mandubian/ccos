(ns rtfs-clojure.parser
  "Parses RTFS S-expressions (as Clojure data) into AST nodes."
  (:require [clojure.edn :as edn]
            [rtfs-clojure.ast :as ast]))

;; Forward declaration for mutual recursion if needed (though maybe not strictly necessary here)
(declare parse-expr)

(defn- parse-error [msg data]
  (throw (ex-info msg {:type :rtfs-parse-error :data data})))

(defn- parse-symbol [data]
  (if (symbol? data)
    (ast/make-symbol data)
    (parse-error "Expected symbol" data)))

(defn- parse-let-bindings [bindings-vec]
  (if (and (vector? bindings-vec) (even? (count bindings-vec)))
    (->> bindings-vec
         (partition 2)
         (mapv (fn [[sym-expr val-expr]]
                 (ast/make-let-binding (parse-symbol sym-expr) (parse-expr val-expr)))))
    (parse-error "let bindings must be a vector with an even number of elements" bindings-vec)))

(defn- parse-parallel-bindings [bindings-vec]
  (if (and (vector? bindings-vec) (every? vector? bindings-vec))
    (mapv (fn [[id-sym expr]]
            (ast/make-parallel-binding (parse-symbol id-sym) (parse-expr expr)))
          bindings-vec)
    (parse-error "parallel bindings must be a vector of [id expr] vectors" bindings-vec)))

(defn- parse-fn-params [params-vec]
  (if (vector? params-vec)
    (mapv parse-symbol params-vec)
    (parse-error "fn params must be a vector of symbols" params-vec)))

(defn parse-expr [data]
  (cond
    ;; Literals
    (or (number? data) (string? data) (boolean? data) (nil? data) (keyword? data))
    (ast/make-literal data)

    ;; Symbol (Variable Lookup)
    (symbol? data)
    (ast/make-symbol data)

    ;; List (Special Forms or Function Call)
    (list? data)
    (if (empty? data)
      (parse-error "Cannot parse empty list" data)
      (let [op (first data)
            args (rest data)]
        (condp = op
          'def (if (= 2 (count args))
                 (ast/make-def (parse-symbol (first args)) (parse-expr (second args)))
                 (parse-error "def requires a symbol and one value expression" data))

          'let (if (>= (count args) 1)
                 (let [bindings-vec (first args)
                       body (rest args)]
                   (ast/make-let (parse-let-bindings bindings-vec) (mapv parse-expr body)))
                 (parse-error "let requires bindings and at least one body expression" data))

          'if (if (= 3 (count args))
                (ast/make-if (parse-expr (nth args 0))
                             (parse-expr (nth args 1))
                             (parse-expr (nth args 2)))
                (parse-error "if requires condition, then-branch, and else-branch" data))

          'fn (if (>= (count args) 1)
                (let [params-vec (first args)
                      body (rest args)]
                  (ast/make-fn (parse-fn-params params-vec) (mapv parse-expr body)))
                (parse-error "fn requires params vector and at least one body expression" data))

          'do (ast/make-do (mapv parse-expr args))

          'parallel (if (= 1 (count args))
                      (ast/make-parallel (parse-parallel-bindings (first args)))
                      (parse-error "parallel requires a single vector of bindings" data))

          'join (ast/make-join (mapv parse-symbol args))

          'log-step (if (and (= 3 (count args)) (= :id (first args))) ; Expecting (log-step :id <id-sym> <expr>)
                      (ast/make-log-step (parse-symbol (second args)) (parse-expr (nth args 2)))
                      (parse-error "log-step requires :id <symbol> <expr>" data))

          ;; Default: Function/Tool Call
          (ast/make-call (mapv parse-expr data))))) ; Parse the whole list including the operator symbol

    ;; Vector (Treat as literal for now)
    (vector? data)
    (ast/make-literal data)

    ;; Map
    (map? data)
    (if (and (contains? data :plan) (contains? data :id)) ; Heuristic: If it has :plan and :id, treat as Task
      (ast/make-task
       :id (:id data) ; Assuming id is simple value for now
       :source (:source data)
       :natural-language (:natural-language data)
       :intent (:intent data) ; Keep intent as data
       :plan (parse-expr (:plan data)) ; Recursively parse the plan
       :execution-log (:execution-log data)) ; Keep log as data for now
      (ast/make-literal data)) ; Otherwise, treat as a generic literal map

    :else
    (parse-error "Cannot parse input into RTFS AST" data)))


(defn parse-rtfs-string
  "Parses an RTFS string into an AST node.
   Uses clojure.edn/read-string for safety."
  [s]
  (try
    (let [data (edn/read-string s)]
      (parse-expr data))
    (catch Exception e
      (println "Error parsing RTFS string:" (.getMessage e))
      ;; Consider re-throwing or returning a more structured error
      (throw (ex-info (str "Failed to parse RTFS string: " (.getMessage e))
                      {:type :rtfs-parse-error :input-string s}
                      e)))))

;; Example Usage (for REPL testing)
(comment
  (parse-rtfs-string "42")
  (parse-rtfs-string "\"hello\"")
  (parse-rtfs-string ":my-keyword")
  (parse-rtfs-string "my-symbol")
  (parse-rtfs-string "(def x 10)")
  (parse-rtfs-string "(let [a 1 b \"two\"] (do (print a) b))")
  (parse-rtfs-string "(if (> x 5) \"big\" \"small\")")
  (parse-rtfs-string "(fn [y] (+ y 1))")
  (parse-rtfs-string "(do (println \"hi\") (+ 1 2))")
  (parse-rtfs-string "(parallel [[job1 (tool:fetch \"A\")] [job2 (tool:fetch \"B\")]] )") ; Note: space needed before closing ) for edn/read-string
  (parse-rtfs-string "(join job1 job2)")
  (parse-rtfs-string "(log-step :id step1 (call-tool param))")
  (parse-rtfs-string "(some-func arg1 :named-arg val)")
  (parse-rtfs-string "[1 2 3]") ; Vector literal
  (parse-rtfs-string "{:a 1 :b 2}") ; Map literal
  )

;; Error cases
  ; (parse-rtfs-string "(")
  ; (parse-rtfs-string "(def x)")
  ; (parse-rtfs-string "(let [a] a)")
  ; (parse-rtfs-string "(parallel [a 1] [b 2])") ; Incorrect structure for parallel bindings
  ; (parse-rtfs-string "(log-step step1 expr)")
  ; Close the comment block properly
