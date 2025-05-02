(ns rtfs-clojure.parser-test
  (:require [clojure.test :refer :all]
            [rtfs-clojure.parser :as parser]
            [rtfs-clojure.ast :as ast]))

(deftest literal-symbol-parsing-test
  (testing "Parsing basic literals and symbols"
    (is (instance? rtfs_clojure.ast.Literal (parser/parse-rtfs-string "123")))
    (is (= 123 (:value (parser/parse-rtfs-string "123"))))
    (is (instance? rtfs_clojure.ast.Literal (parser/parse-rtfs-string "\"hello\"")))
    (is (= "hello" (:value (parser/parse-rtfs-string "\"hello\""))))
    (is (instance? rtfs_clojure.ast.Literal (parser/parse-rtfs-string "true")))
    (is (= true (:value (parser/parse-rtfs-string "true"))))
    (is (instance? rtfs_clojure.ast.Literal (parser/parse-rtfs-string ":keyword")))
    (is (= :keyword (:value (parser/parse-rtfs-string ":keyword"))))
    (is (instance? rtfs_clojure.ast.Symbol (parser/parse-rtfs-string "my-var")))
    (is (= "my-var" (:name (parser/parse-rtfs-string "my-var"))))))

(deftest simple-forms-parsing-test
  (testing "Parsing simple special forms and calls"
    (is (instance? rtfs_clojure.ast.Def (parser/parse-rtfs-string "(def x 1)")))
    (is (instance? rtfs_clojure.ast.Let (parser/parse-rtfs-string "(let [a 1] a)")))
    (is (instance? rtfs_clojure.ast.If (parser/parse-rtfs-string "(if true 1 2)")))
    (is (instance? rtfs_clojure.ast.Fn (parser/parse-rtfs-string "(fn [x] x)")))
    (is (instance? rtfs_clojure.ast.Do (parser/parse-rtfs-string "(do (print 1))")))
    (is (instance? rtfs_clojure.ast.Parallel (parser/parse-rtfs-string "(parallel [[a (f 1)]])")))
    (is (instance? rtfs_clojure.ast.Join (parser/parse-rtfs-string "(join a b)")))
    (is (instance? rtfs_clojure.ast.LogStep (parser/parse-rtfs-string "(log-step :id s1 (f 1))")))
    (is (instance? rtfs_clojure.ast.Call (parser/parse-rtfs-string "(my-func 1 2)")))))


(deftest full-task-parsing-test
  (testing "Parsing a full realistic RTFS task map"
    (let [task-string "{:id \"task-001\"
                      :source :human-instruction
                      :natural-language \"Fetch data, process it, aggregate results.\"
                      :intent {:action :complex-data-pipeline}
                      :plan (do
                              (def process-item
                                (fn [item-val]
                                  (if (> item-val 10)
                                    (tool:process-large item-val)
                                    (tool:process-small item-val))))
                              (log-step :id fetch-parallel
                                (parallel
                                  [[data-a (tool:fetch \"source-a\")]
                                   [data-b (tool:fetch \"source-b\")]]))
                              (let [results (join data-a data-b)
                                    processed-a (log-step :id proc-a (process-item 15))
                                    processed-b (log-step :id proc-b (process-item 5))]
                                (tool:aggregate processed-a processed-b)))
                      :execution-log [{:stage 1 :status :received}]}"
          parsed-task (parser/parse-rtfs-string task-string)]

      ;; Check top-level Task structure
      (is (instance? rtfs_clojure.ast.Task parsed-task) "Top level should be Task")
      (is (= "task-001" (:id parsed-task)))
      (is (= :human-instruction (:source parsed-task)))
      (is (= "Fetch data, process it, aggregate results." (:natural-language parsed-task)))
      (is (= {:action :complex-data-pipeline} (:intent parsed-task)))
      (is (= [{:stage 1 :status :received}] (:execution-log parsed-task)))

      ;; Check the parsed plan within the task
      (let [parsed-plan (:plan parsed-task)]
        (is (instance? rtfs_clojure.ast.Do parsed-plan) "Plan should be Do")
        (is (= 3 (count (:body parsed-plan))) "Plan's Do should have 3 main expressions")

        ;; Check first expression in plan (Def)
        (let [def-node (first (:body parsed-plan))]
          (is (instance? rtfs_clojure.ast.Def def-node) "First plan expr should be Def")
          (is (= "process-item" (:name (:symbol def-node))))
          (is (instance? rtfs_clojure.ast.Fn (:value def-node)) "Def value should be Fn"))

        ;; Check second expression in plan (LogStep containing Parallel)
        (let [log-step-node (second (:body parsed-plan))]
          (is (instance? rtfs_clojure.ast.LogStep log-step-node) "Second plan expr should be LogStep")
          (is (= "fetch-parallel" (:name (:id log-step-node))))
          (is (instance? rtfs_clojure.ast.Parallel (:expr log-step-node)) "LogStep expr should be Parallel"))

        ;; Check third expression in plan (Let)
        (let [let-node (nth (:body parsed-plan) 2)]
          (is (instance? rtfs_clojure.ast.Let let-node) "Third plan expr should be Let")
          (is (= 3 (count (:bindings let-node))) "Let should have 3 bindings")
          (is (= 1 (count (:body let-node))) "Let should have 1 body expression")
          (let [let-body-expr (first (:body let-node))]
            (is (instance? rtfs_clojure.ast.Call let-body-expr) "Let body should be a Call")
            (is (= "tool:aggregate" (:name (first (:elements let-body-expr)))))))))))

(comment
  ;; To run tests from REPL:
  (require 'clojure.test)
  (clojure.test/run-tests 'rtfs-clojure.parser-test))
