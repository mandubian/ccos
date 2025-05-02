(ns rtfs-clojure.ast-test
  (:require [clojure.test :refer :all]
            [rtfs-clojure.ast :as ast]))

(deftest literal-creation-test
  (testing "Creating Literal nodes."
    (is (= (ast/->Literal 123) (ast/make-literal 123)))
    (is (= (ast/->Literal "hello") (ast/make-literal "hello")))
    (is (= (ast/->Literal true) (ast/make-literal true)))
    (is (= (ast/->Literal :kw) (ast/make-literal :kw)))
    (is (= (ast/->Literal nil) (ast/make-literal nil)))))

(deftest symbol-creation-test
  (testing "Creating Symbol nodes."
    (is (= (ast/->Symbol "my-var") (ast/make-symbol "my-var")))
    (is (= (ast/->Symbol "my-var") (ast/make-symbol 'my-var))) ; Ensure conversion from symbol
    (is (= (ast/->Symbol "my-var") (ast/make-symbol :my-var))))) ; Ensure conversion from keyword

(deftest call-creation-test
  (testing "Creating Call nodes."
    (let [elements [(ast/make-symbol '+) (ast/make-literal 1) (ast/make-literal 2)]]
      (is (= (ast/->Call elements) (ast/make-call elements))))))

(deftest do-creation-test
  (testing "Creating Do nodes."
    (let [body [(ast/make-call [(ast/make-symbol 'println) (ast/make-literal "hello")])]]
      (is (= (ast/->Do body) (ast/make-do body))))))

(deftest parallel-creation-test
  (testing "Creating Parallel nodes."
    (let [binding1 (ast/make-parallel-binding (ast/make-symbol 'a) (ast/make-literal 1))
          binding2 (ast/make-parallel-binding (ast/make-symbol 'b) (ast/make-literal 2))
          bindings [binding1 binding2]]
      (is (= (ast/->Parallel bindings) (ast/make-parallel bindings)))
      (is (= (ast/->ParallelBinding (ast/->Symbol "a") (ast/->Literal 1)) binding1)))))

(deftest join-creation-test
  (testing "Creating Join nodes."
    (let [ids [(ast/make-symbol 'a) (ast/make-symbol 'b)]]
      (is (= (ast/->Join ids) (ast/make-join ids))))))

(deftest log-step-creation-test
  (testing "Creating LogStep nodes."
    (let [id (ast/make-symbol 'step1)
          expr (ast/make-literal "done")]
      (is (= (ast/->LogStep id expr) (ast/make-log-step id expr))))))

(deftest log-entry-creation-test
  (testing "Creating LogEntry nodes."
    (let [entry (ast/make-log-entry :stage :planning :agent "planner" :status :success)]
      (is (= :planning (:stage entry)))
      (is (= "planner" (:agent entry)))
      (is (= :success (:status entry)))
      (is (nil? (:timestamp entry)))))) ; Check default nil

(deftest task-creation-test
  (testing "Creating Task nodes."
    (let [task1 (ast/make-task :id "task-123" :source "human")
          plan [(ast/make-log-step (ast/make-symbol 's1) (ast/make-literal "step 1"))]
          log [(ast/make-log-entry :stage :exec :status :done)]
          task2 (ast/make-task :id "task-456" :plan plan :execution-log log)]
      (is (= "task-123" (:id task1)))
      (is (= "human" (:source task1)))
      (is (= {} (:intent task1))) ; Check default
      (is (= [] (:plan task1))) ; Check default
      (is (= [] (:execution-log task1))) ; Check default

      (is (= "task-456" (:id task2)))
      (is (= plan (:plan task2)))
      (is (= log (:execution-log task2))))))
