(ns rtfs-clojure.core
  (:require [rtfs-clojure.ast :as ast]
            [rtfs-clojure.parser :as parser]
            [clojure.string :as str])
  (:gen-class))

(println "RTFS Clojure Core Loaded!")

;; TODO: Implement RTFS parser, typechecker, interpreter

(defn -main [& args]
  (println "Running RTFS Clojure main...")
  (if (first args)
    (let [input-string (str/join " " args)
          ;; Parser needs to be updated for the new AST
          ;; parsed-message (parser/parse-acl-string input-string)
          ]
      (println "Input:" input-string)
      ;; (println "Parsed:" parsed-message)
      (println "Parser needs update for RTFS AST."))
    (println "Usage: lein run \"<rtfs expression>\"")))

;; Example:
;; lein run "(some-rtfs-expression)"
