# RTFS 2.0: Security Model

## 1. Security Overview

RTFS implements a security model based on capability-based security, where access to resources and operations is granted through unforgeable capability tokens. The model ensures that all potentially dangerous operations are mediated through the host boundary.

### Core Principles

- **Capability-Based Access**: No ambient authority, explicit capabilities required
- **Least Privilege**: Minimal capabilities granted for specific operations
- **Host Mediation**: All external interactions go through secure host interface

## 2. Capability System

### Core Capability Types

```rtfs
;; File system capabilities
:fs.read      ; Read files
:fs.write     ; Write/modify files

;; Network capabilities
:net.http.get     ; HTTP GET requests
:net.http.post    ; HTTP POST requests

;; System capabilities
:sys.time         ; Access system time
```

### Capability Tokens

```rtfs
;; Request a capability
(def read-cap (request-capability :fs.read))

;; Check capability
(has-capability? read-cap)  ; true if granted

;; Use capability
(with-capability read-cap
  (read-file "/file.txt"))
```

## 3. Host Boundary Security

### Host Calls

```rtfs
;; Secure host call
(def result (host-call :fs.read {:path "/file.txt"}))

;; Host call with capability
(with-capability fs-cap
  (host-call :fs.write {:path "/file.txt" :content "data"}))
```

### Host Interface Definition

```rtfs
;; Host function signature
(def-host-fn read-file
  {:capability :fs.read
   :parameters {:path String}
   :return String})
```

This security model provides essential protection through capability-based access control and host boundary mediation, maintaining RTFS's functional purity while enabling secure external interactions.