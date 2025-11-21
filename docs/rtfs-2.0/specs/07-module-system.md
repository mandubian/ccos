# RTFS 2.0: Module System

## 1. Module System Overview

RTFS provides a comprehensive module system for organizing code into reusable, composable units. The module system supports explicit imports/exports, namespace isolation, and dependency management.

### Core Principles

- **Explicit Interfaces**: Clear boundaries between modules
- **Namespace Isolation**: Prevent naming conflicts
- **Dependency Tracking**: Automatic dependency resolution
- **Incremental Loading**: Load modules on demand

## 2. Module Definition

### Basic Module Structure

```rtfs
;; Module declaration with exports
(module my.app/math
  (:exports [add multiply PI])
  
  ;; Import declarations - dependencies
  (import my.app/core :as core)
  (import my.app/string :only [join split])

  ;; Module body - implementation
  (def PI 3.14159)

  (defn add [a b]
    (core/+ a b))

  (defn multiply [a b]
    (core/* a b))

  ;; Private functions (not in exports list)
  (defn helper [x]
    (* x 2)))
```

### Module Types

#### Library Modules

```rtfs
;; Pure library - only exports, no side effects
(module math.utils
  (:exports [gcd lcm fibonacci])

  (defn gcd [a b]
    (if (= b 0)
        a
        (gcd b (mod a b))))

  (defn lcm [a b]
    (/ (* a b) (gcd a b)))

  (defn fibonacci [n]
    (if (< n 2)
        n
        (+ (fibonacci (- n 1))
           (fibonacci (- n 2))))))
```

#### Application Modules

```rtfs
;; Application module - may have side effects
(module app.main
  (import http.server :as server)
  (import db.connection :as db)
  (import app.routes :as routes)

  (defn start []
    (db/connect)
    (server/start routes/handler))

  (defn stop []
    (server/stop)
    (db/disconnect)))
```

#### Macro Modules

```rtfs
;; Module providing macros
(module my-macros
  (export defhandler unless)

  (defmacro defhandler [name args & body]
    `(defn ~name ~args
       (try
         ~@body
         (catch Exception e
           (log-error e)
           {:status 500 :body "Internal error"}))))

  (defmacro unless [test & body]
    `(if (not ~test)
         (do ~@body))))
```

## 3. Import and Export Mechanisms

### Export Declarations

```rtfs
;; Export specific symbols
(export add multiply subtract)

;; Export all public symbols (discouraged for libraries)
(export :all)

;; Conditional exports
(export
  add multiply
  (when *debug* debug-helper))
```

### Import Declarations

```rtfs
;; Aliased import
(import [math :as m])
;; Usage: (m/sqrt 16)

;; Selective import
(import [collections :refer [map filter reduce]])
;; Usage: (map inc [1 2 3])

;; Qualified import
(import [http.client])
;; Usage: (http.client/get "https://api.example.com")

;; Renamed import
(import [old.module :as new-name])
;; Usage: (new-name/function)
```

### Import Resolution

1. **Local modules**: Check current project
2. **Standard library**: Built-in RTFS modules
3. **External dependencies**: Configured repositories
4. **Dynamic loading**: Load from network/cache

## 4. Namespace Management

### Qualified Names

```rtfs
;; Fully qualified names
(math.utils/gcd 12 18)  ; => 6

;; Namespace navigation
(ns my.app.core)
(require '[my.app.utils :as utils])
(utils/helper-function)
```

### Namespace Aliases

```rtfs
;; Create namespace alias
(alias 'm 'my.math.library)

;; Use alias
(m/add 1 2)  ; Equivalent to my.math.library/add
```

### Name Resolution Rules

1. **Local bindings**: Function parameters, let bindings
2. **Current namespace**: Definitions in current module
3. **Imports**: Aliased and referred symbols
4. **Fully qualified**: Explicit namespace paths
5. **Host capabilities**: External operations

## 5. Module Dependencies and Loading

### Dependency Declaration

```rtfs
;; Module with dependencies
(module data.processor
  (import [http.client :as http]
          [json.parser :as json]
          [db.connection :as db])

  ;; Dependencies loaded automatically
  (defn fetch-and-process [url]
    (let [response (http/get url)
          data (json/parse (:body response))]
      (db/save data))))
```

### Lazy Loading

```rtfs
;; Load module on first use
(require '[heavy.library :as heavy])

(defn use-heavy-lib []
  ;; Module loaded here when first called
  (heavy/expensive-operation))
```

### Circular Dependency Prevention

```rtfs
;; This would cause an error
(module A (import [B]))
(module B (import [A]))  ; Circular dependency detected

;; Solution: Extract common interface
(module common (export Protocol))
(module A (import [common] [B]))
(module B (import [common] [A]))
```

## 6. Module Metadata and Introspection

### Module Information

```rtfs
;; Get module metadata
(module-info 'my-module)
;; Returns: {:name "my-module"
;;           :exports [add multiply]
;;           :imports [math string]
;;           :version "1.0.0"}

;; List loaded modules
(loaded-modules)
;; Returns: ["rtfs.core" "my-module" "math.utils"]

;; Check if module is loaded
(module-loaded? 'collections)
;; Returns: true
```

### Runtime Module Manipulation

```rtfs
;; Load module at runtime
(load-module 'dynamic.feature)

;; Reload module (development)
(reload-module 'my-module)

;; Unload module
(unload-module 'unused.module)
```

## 7. Module Testing and Validation

### Module Testing

```rtfs
;; Module with tests
(module math.utils
  (export add multiply)

  (defn add [a b] (+ a b))
  (defn multiply [a b] (* a b))

  ;; Test declarations
  (deftest test-add
    (assert (= (add 1 2) 3))
    (assert (= (add -1 1) 0)))

  (deftest test-multiply
    (assert (= (multiply 3 4) 12))
    (assert (= (multiply 0 10) 0))))
```

### Module Validation

```rtfs
;; Validate module structure
(validate-module 'my-module)
;; Checks: exports exist, imports resolve, no circular deps

;; Lint module
(lint-module 'my-module)
;; Checks: unused imports, naming conventions, documentation
```

## 8. Module Distribution and Packaging

### Module Packaging

```rtfs
;; Module descriptor
{:name "my-library"
 :version "1.0.0"
 :description "Useful utilities"
 :author "Developer"
 :license "MIT"
 :dependencies {"rtfs.core" "2.0.0"
                "collections" "1.5.0"}
 :main 'my-library.core
 :exports ["helper" "utils"]}
```

### Repository Management

```rtfs
;; Install from repository
(install-module 'awesome.lib "1.2.0")

;; Update dependencies
(update-dependencies)

;; List installed modules
(list-modules)
;; Shows: name, version, status, dependencies
```

## 9. Advanced Module Features

### Conditional Modules

```rtfs
;; Platform-specific modules
(module fs.utils
  (export read-file write-file)

  (cond-platform
    :unix
    (defn read-file [path]
      (unix/read-file path))

    :windows
    (defn read-file [path]
      (windows/read-file path))))
```

### Dynamic Module Generation

```rtfs
;; Generate module at runtime
(define-dynamic-module 'generated.module
  {:exports '[dynamic-fn]
   :code '(defn dynamic-fn [] "Hello from generated module")})
```

### Module Hooks

```rtfs
;; Module lifecycle hooks
(module my-module
  (on-load
    (println "Module loading..."))

  (on-unload
    (cleanup-resources))

  (export my-function)

  (defn my-function []
    "Module function"))
```

## 10. Module Security

### Sandboxing

```rtfs
;; Restricted module execution
(with-sandbox {:allowed-capabilities [:fs.read :http.get]}
  (load-module 'untrusted.code))
```

### Capability Declaration

```rtfs
;; Module declares required capabilities
(module network.client
  (capabilities [:http.get :http.post :dns.resolve])

  (export make-request)

  (defn make-request [url]
    (http/get url)))
```

### Trust Levels

```rtfs
;; Trust levels for module execution
{:trusted     - Full access
 :standard    - Limited capabilities
 :sandboxed   - Minimal access
 :untrusted   - No external access}
```

## 11. Performance Optimization

### Module Caching

```rtfs
;; Cache compiled modules
(enable-module-cache "/tmp/rtfs-cache")

;; Precompile modules
(precompile-module 'performance.critical)

;; Lazy compilation
(lazy-compile-modules)
```

### Memory Management

```rtfs
;; Unload unused modules
(auto-unload-modules {:ttl 3600000})  ; 1 hour

;; Module memory limits
(set-module-memory-limit 'large.module (* 100 1024 1024))  ; 100MB
```

## 12. Development Workflow

### Module Development

```rtfs
;; Create new module
(create-module 'my.new.module)

;; Add dependency
(add-dependency 'my.module 'useful.lib "1.0.0")

;; Test module
(test-module 'my.module)

;; Publish module
(publish-module 'my.module {:repository "company-repo"})
```

### REPL Integration

```rtfs
;; Load module in REPL
(use 'my.module)

;; Reload after changes
(reload)

;; Inspect module
(doc 'my-function)
(source 'my-function)
```

## 13. Implementation Architecture

### Module Registry

```rust
struct ModuleRegistry {
    loaded: HashMap<String, Module>,
    cache: ModuleCache,
    resolver: DependencyResolver,
}
```

### Module Structure

```rust
struct Module {
    name: String,
    exports: Vec<String>,
    imports: Vec<Import>,
    definitions: HashMap<String, Value>,
    metadata: ModuleMetadata,
}
```

### Loading Process

1. **Resolve dependencies** recursively
2. **Load module code** from source/cache
3. **Compile definitions** in dependency order
4. **Register exports** in global namespace
5. **Execute initialization** code

This comprehensive module system enables RTFS to scale from small scripts to large, maintainable applications while providing safety, composability, and performance.