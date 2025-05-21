(defproject rtfs-clojure "0.1.0-SNAPSHOT"
  :description "RTFS (Reasoning Task Flow Specification) parser and interpreter in Clojure"
  :url "http://example.com/FIXME" ; Placeholder URL
  :license {:name "EPL-2.0 OR GPL-2.0-or-later WITH Classpath-exception-2.0"
            :url "https://www.eclipse.org/legal/epl-2.0/"} ; Example license
  :dependencies [[org.clojure/clojure "1.11.1"]] ; Specify Clojure version
  :source-paths ["src"]
  :test-paths ["test"]
  :main rtfs-clojure.core ; Specify the main namespace for 'lein run'
  :profiles {:dev {:dependencies [[org.clojure/test.check "1.1.1"]]}} ; Dev dependencies
  :repl-options {:init-ns rtfs-clojure.core})
