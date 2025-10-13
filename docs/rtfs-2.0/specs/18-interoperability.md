# RTFS 2.0: Interoperability

## 1. Interoperability Overview

RTFS provides interoperability with external systems through the host boundary, enabling secure data exchange and external operations while maintaining functional purity.

### Core Principles

- **Host Mediation**: All external interactions go through secure host interface
- **Capability-Based Access**: External operations require explicit capabilities
- **Data Exchange**: Structured data exchange between RTFS and host systems

## 2. Host Boundary Integration

### Host Calls

```rtfs
;; File system operations through host
(host-call :fs.read {:path "/file.txt"})

;; Network operations through host
(host-call :net.http.get {:url "https://api.example.com"})

;; System operations through host
(host-call :sys.time {})
```

### Data Exchange Formats

RTFS uses structured data for host communication:

```rtfs
;; Request format
{:operation :fs.read
 :parameters {:path "/file.txt" :encoding "utf-8"}}

;; Response format
{:result "file contents"
 :status :success}

;; Error format
{:error "File not found"
 :status :error}
```

## 3. External Data Integration

### Basic Data Exchange

```rtfs
;; Reading external data
(def file-data (host-call :fs.read {:path "data.json"}))
(def parsed (parse-json file-data))  ; If JSON parsing exists

;; Writing data
(host-call :fs.write {:path "output.txt" :content "data"})
```

### Structured Communication

```rtfs
;; HTTP requests
(def response (host-call :net.http.get
  {:url "https://api.example.com/users"
   :headers {"Accept" "application/json"}}))

;; Response handling
(if (= (:status response) :success)
  (process-data (:body response))
  (handle-error (:error response)))
```

This interoperability approach ensures secure, controlled interaction with external systems through the host boundary while maintaining RTFS's functional and security guarantees.