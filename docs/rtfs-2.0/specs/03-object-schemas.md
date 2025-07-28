# RTFS 2.0 Object Schemas Specification

**Status:** Stable  
**Version:** 2.0.0  
**Date:** July 2025  
**Implementation:** Complete

## 1. Overview

This document defines the formal object schemas for RTFS 2.0, specifying the structure, validation rules, and relationships between all core objects in the system. These schemas serve as the foundation for type safety, validation, and interoperability across the RTFS 2.0 ecosystem.

## 2. Core Object Types

RTFS 2.0 defines five primary object types that form the foundation of the system:

1. **Capability** - Executable functions with attestation
2. **Intent** - User goals and objectives
3. **Plan** - Executable RTFS code
4. **Action** - Recorded execution events
5. **Resource** - Managed system resources

## 3. Intent Schema

### 3.1 Core Intent Structure

```clojure
(intent
  :type :rtfs.core:v2.0:intent,
  :intent-id "intent-uuid-12345",
  :goal "Analyze quarterly sales performance and create executive summary",
  :created-at "2025-06-23T10:30:00Z",
  :created-by "user:alice@company.com",
  :priority :high,
  :constraints {
    :max-cost 25.00,
    :deadline "2025-06-25T17:00:00Z",
    :data-locality [:US, :EU],
    :security-clearance :confidential,
    :preferred-style :executive-formal
  },
  :success-criteria (fn [result] 
    (and (contains? result :executive-summary)
         (contains? result :key-metrics)
         (> (:confidence result) 0.85))),
  :parent-intent "intent-uuid-9876",
  :child-intents ["intent-uuid-11111", "intent-uuid-22222"],
  :status :active,
  :metadata {
    :department "sales",
    :quarter "Q2-2025",
    :stakeholders ["ceo@company.com", "cfo@company.com"]
  }
)
```

### 3.2 Intent Schema Definition

```clojure
{:type [:required :keyword]                  ; Object type identifier
 :intent-id [:required :string]              ; Unique identifier
 :goal [:required :string]                   ; Human-readable description
 :created-at [:required :timestamp]          ; Creation timestamp
 :created-by [:required :string]             ; Creator identity
 :priority [:optional [:enum [:low :normal :high :urgent :critical]]]
 :constraints [:optional :map]               ; Execution constraints
 :success-criteria [:optional :function]     ; Success validation function
 :parent-intent [:optional :string]          ; Parent intent reference
 :child-intents [:optional [:vector :string]] ; Child intent references
 :status [:required [:enum [:draft :active :paused :completed :failed :archived]]]
 :metadata [:optional :map]}                 ; Additional metadata
```

## 4. Plan Schema

### 4.1 Core Plan Structure

```clojure
(plan
  :type :rtfs.core:v2.0:plan,
  :plan-id "plan-uuid-67890",
  :created-at "2025-06-23T10:35:00Z",
  :created-by :arbiter,
  :intent-ids ["intent-uuid-12345"],
  :input-schema {
    :sales-quarter [:required :string "Q[1-4]-\\d{4}"]
  },
  :output-schema {
    :executive-summary [:required :document]
    :key-metrics [:required :map]
  },
  :strategy :scripted-execution,
  :estimated-cost 18.50,
  :estimated-duration 1800, ; seconds
  :program (do
    ;; This is now an executable RTFS program.
    ;; The 'call' function is special: it invokes a capability and logs an Action.
    
    ;; Step 1: Fetch data and bind the output resource to a variable.
    (let [sales_data (call :com.acme.db:v1.0:sales-query 
                           {:query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
                            :format :csv})]
      
      ;; Step 2: Analyze the data using the resource from the previous step.
      (let [summary_document (call :com.openai:v1.0:data-analysis
                                   {:data sales_data,
                                    :analysis-type :quarterly-summary,
                                    :output-format :executive-brief})]
        
        ;; The final expression of the 'do' block is the plan's result.
        summary_document
      )
    )
  ),
  :status :ready,
  :execution-context {
    :arbiter-reasoning "Generated a scripted plan for maximum flexibility.",
    :alternative-strategies [:declarative-dag, :sequential-steps],
    :risk-assessment :low
  }
)
```

### 4.2 Plan Schema Definition

```clojure
{:type [:required :keyword]                  ; Object type identifier
 :plan-id [:required :string]                ; Unique identifier
 :created-at [:required :timestamp]          ; Creation timestamp
 :created-by [:required :string]             ; Creator (usually :arbiter)
 :intent-ids [:required [:vector :string]]   ; Associated intent IDs
 :input-schema [:optional :map]              ; Input validation schema
 :output-schema [:optional :map]             ; Output validation schema
 :strategy [:optional :keyword]              ; Execution strategy
 :estimated-cost [:optional :number]         ; Cost estimate
 :estimated-duration [:optional :number]     ; Duration estimate (seconds)
 :program [:required :rtfs-expression]       ; Executable RTFS program
 :status [:required [:enum [:draft :ready :executing :completed :failed]]]
 :execution-context [:optional :map]}        ; Execution metadata
```

## 5. Action Schema

### 5.1 Core Action Structure

```clojure
(action
  :type :rtfs.core:v2.0:action,
  :action-id "action-uuid-54321",
  :timestamp "2025-06-23T10:37:15Z",
  :plan-id "plan-uuid-67890",
  :step-id "step-1",
  :intent-id "intent-uuid-12345",
  :capability-used :com.acme.db:v1.0:sales-query,
  :executor {
    :type :agent,
    :id "agent-db-cluster-1",
    :node "node.us-west.acme.com"
  },
  :input {
    :query "SELECT * FROM sales WHERE quarter = 'Q2-2025'",
    :format :csv
  },
  :output {
    :type :resource,
    :handle "resource://sales-data-q2-2025.csv",
    :size 2048576,
    :checksum "sha256:abc123...",
    :metadata {
      :rows 15234,
      :columns 12,
      :data-quality-score 0.94
    }
  },
  :execution {
    :started-at "2025-06-23T10:37:15Z",
    :completed-at "2025-06-23T10:37:18Z",
    :duration 3.2,
    :cost 0.45,
    :status :success
  },
  :signature {
    :signed-by "arbiter-key-hash",
    :signature "crypto-signature-xyz",
    :algorithm "ed25519"
  }
)
```

### 5.2 Action Schema Definition

```clojure
{:type [:required :keyword]                  ; Object type identifier
 :action-id [:required :string]              ; Unique identifier
 :timestamp [:required :timestamp]           ; Execution timestamp
 :plan-id [:required :string]                ; Associated plan ID
 :step-id [:optional :string]                ; Step identifier
 :step-name [:optional :string]              ; Step name (for step actions)
 :intent-id [:required :string]              ; Associated intent ID
 :operation [:required [:enum [:plan-step-started :plan-step-completed :plan-step-failed :capability-execution :intent-creation :error]]]
 :capability-used [:optional :keyword]       ; Used capability (for capability actions)
 :executor [:required :map]                  ; Executor information
 :input [:optional :map]                     ; Input data
 :output [:optional :map]                    ; Output data
 :execution [:required :map]                 ; Execution metadata
 :signature [:required :map]}                ; Cryptographic signature
```

### 5.3 Step Action Examples

```clojure
;; Plan Step Started Action
(action
  :type :rtfs.core:v2.0:action,
  :action-id "action-uuid-54321",
  :timestamp "2025-06-23T10:37:15Z",
  :operation :plan-step-started,
  :step-name "fetch-sales-data",
  :plan-id "plan-uuid-67890",
  :intent-id "intent-uuid-12345",
  :executor {
    :type :arbiter,
    :id "arbiter-1",
    :node "node.us-west.acme.com"
  },
  :execution {
    :started-at "2025-06-23T10:37:15Z",
    :status :started
  },
  :signature {
    :signed-by "arbiter-key-hash",
    :signature "crypto-signature-xyz",
    :algorithm "ed25519"
  }
)

;; Plan Step Completed Action
(action
  :type :rtfs.core:v2.0:action,
  :action-id "action-uuid-54322",
  :timestamp "2025-06-23T10:37:18Z",
  :operation :plan-step-completed,
  :step-name "fetch-sales-data",
  :plan-id "plan-uuid-67890",
  :intent-id "intent-uuid-12345",
  :output {
    :type :resource,
    :handle "resource://sales-data-q2-2025.csv",
    :size 2048576
  },
  :executor {
    :type :arbiter,
    :id "arbiter-1",
    :node "node.us-west.acme.com"
  },
  :execution {
    :started-at "2025-06-23T10:37:15Z",
    :completed-at "2025-06-23T10:37:18Z",
    :duration 3.0,
    :status :success
  },
  :signature {
    :signed-by "arbiter-key-hash",
    :signature "crypto-signature-abc",
    :algorithm "ed25519"
  }
)
```

## 6. Capability Schema

### 6.1 Core Capability Structure

```clojure
(capability
  :type :rtfs.core:v2.0:capability,
  :capability-id :com.acme.db:v1.0:sales-query,
  :created-at "2025-06-20T09:00:00Z",
  :provider {
    :name "ACME Database Services",
    :contact "support@acme.com",
    :node-id "node.us-west.acme.com",
    :reputation 4.8,
    :certifications [:iso-27001, :soc2-type2]
  },
  :function {
    :name "sales-query",
    :description "Execute SQL queries against the sales data warehouse",
    :signature {
      :inputs {
        :query [:required :string],
        :format [:optional [:enum :csv :json :parquet] :csv]
      },
      :outputs {
        :data :resource,
        :metadata :map
      }
    },
    :examples [
      {
        :input {:query "SELECT COUNT(*) FROM sales", :format :json},
        :output {:data "resource://example-count.json", :metadata {:rows 1}}
      }
    ]
  },
  :sla {
    :cost-per-call 0.50,
    :max-response-time 10.0, ; seconds
    :availability 0.999,
    :rate-limit {:calls 1000, :period :hour},
    :data-retention 30, ; days
    :geographic-restrictions [:US, :EU, :CA]
  },
  :technical {
    :runtime :postgresql,
    :version "15.3",
    :security [:tls-1.3, :rbac],
    :compliance [:gdpr, :ccpa, :hipaa]
  },
  :status :active,
  :marketplace {
    :listed true,
    :featured false,
    :tags [:database, :sales, :analytics],
    :category :data-access
  }
)
```

### 6.2 Capability Schema Definition

```clojure
{:type [:required :keyword]                  ; Object type identifier
 :capability-id [:required :keyword]         ; Unique capability identifier
 :created-at [:required :timestamp]          ; Creation timestamp
 :provider [:required :map]                  ; Provider information
 :function [:required :map]                  ; Function definition
 :sla [:optional :map]                       ; Service level agreement
 :technical [:optional :map]                 ; Technical specifications
 :status [:required [:enum [:active :deprecated :revoked]]]
 :marketplace [:optional :map]}              ; Marketplace information
```

## 7. Resource Schema

### 7.1 Core Resource Structure

```clojure
(resource
  :type :rtfs.core:v2.0:resource,
  :resource-id "resource-uuid-98765",
  :handle "resource://sales-data-q2-2025.csv",
  :created-at "2025-06-23T10:37:18Z",
  :created-by "action-uuid-54321",
  :content {
    :type :file,
    :mime-type "text/csv",
    :size 2048576,
    :encoding "utf-8",
    :checksum {
      :algorithm :sha256,
      :value "abc123def456..."
    }
  },
  :storage {
    :backend :s3,
    :location "s3://acme-rtfs-resources/2025/06/23/sales-data-q2-2025.csv",
    :region "us-west-2",
    :encryption :aes-256,
    :access-policy :authenticated-read
  },
  :lifecycle {
    :ttl 2592000, ; 30 days in seconds
    :auto-cleanup true,
    :archive-after 604800 ; 7 days
  },
  :metadata {
    :source "ACME Sales Database",
    :description "Q2 2025 sales data export",
    :tags [:sales, :q2-2025, :csv],
    :schema {
      :columns 12,
      :rows 15234,
      :fields [:date, :product_id, :amount, :region, ...]
    }
  },
  :access {
    :permissions [:read],
    :expires-at "2025-07-23T10:37:18Z",
    :accessed-by ["intent-uuid-12345"],
    :access-count 3
  }
)
```

### 7.2 Resource Schema Definition

```clojure
{:type [:required :keyword]                  ; Object type identifier
 :resource-id [:required :string]            ; Unique identifier
 :handle [:required :string]                 ; Resource handle URI
 :created-at [:required :timestamp]          ; Creation timestamp
 :created-by [:required :string]             ; Creator action ID
 :content [:required :map]                   ; Content information
 :storage [:required :map]                   ; Storage information
 :lifecycle [:optional :map]                 ; Lifecycle management
 :metadata [:optional :map]                  ; Content metadata
 :access [:required :map]}                   ; Access control
```





### 4.2 Constraints Schema

```clojure
{:privacy-level [:enum [:public :internal :confidential :secret]]
 :security-level [:enum [:low :medium :high :critical]]
 :timeout-ms [:number :optional]             ; Execution timeout
 :resource-limits [:resource-limits-schema :optional]
 :capability-permissions [:vector :string]   ; Allowed capabilities
 :data-retention [:string :optional]         ; Data retention policy
 :audit-requirements [:vector :string]       ; Audit requirements}
```

### 4.3 Resource Limits Schema

```clojure
{:memory-mb [:number :optional]              ; Memory limit in MB
 :cpu-time-ms [:number :optional]            ; CPU time limit
 :disk-space-mb [:number :optional]          ; Disk space limit
 :network-bandwidth-mb [:number :optional]   ; Network bandwidth limit
 :concurrent-operations [:number :optional]} ; Concurrent operation limit
```

## 5. Plan Schema

### 5.1 Core Plan Structure

```clojure
{:plan-id [:string]                          ; Unique identifier
 :intent-id [:string]                        ; Associated intent
 :version [:string]                          ; Plan version
 :rtfs-code [:rtfs-expression]               ; Executable RTFS code
 :capabilities-required [:vector :string]    ; Required capabilities
 :input-schema [:type-expr]                  ; Input validation
 :output-schema [:type-expr]                 ; Output validation
 :attestation [:attestation-schema]          ; Plan attestation
 :metadata [:map]                            ; Additional metadata
 :created [:timestamp]                       ; Creation timestamp
 :updated [:timestamp]                       ; Last update timestamp
 :status [:enum [:draft :validated :executing :completed :failed]]
 :execution-count [:number]                  ; Number of executions
 :last-executed [:timestamp :optional]       ; Last execution time
 :average-execution-time-ms [:number :optional]}
```

### 5.2 RTFS Expression Schema

```clojure
[:union
 [:literal :any]                             ; Literal values
 

### 7.3 Lifecycle Schema

```clojure
{:auto-cleanup [:boolean]                    ; Auto-cleanup enabled
 :ttl-ms [:number :optional]                 ; Time-to-live in ms
 :max-usage-count [:number :optional]        ; Maximum usage count
 :cleanup-action [:string :optional]         ; Cleanup action
 :retention-policy [:string :optional]}      ; Retention policy
```

## 8. Object Relationships

### 8.1 Dependency Graph

RTFS 2.0 objects form a clear dependency hierarchy:

```
Intent (persistent)
  ↓ generates
Plan (transient → archived)  
  ↓ executes via
Action (immutable) ← uses → Capability (persistent)
  ↓ references
Resource (managed lifecycle)
```

### 8.2 Key Relationships

- **Intent → Plan**: 1:many (one Intent can have multiple Plans over time)
- **Plan → Action**: 1:many (one Plan generates multiple Actions during execution)
- **Action → Resource**: many:many (Actions can create/read multiple Resources)
- **Capability → Action**: 1:many (one Capability can be used by multiple Actions)

## 9. Namespacing and Versioning

### 9.1 Type Namespacing

All objects use the formal type system:
```
:namespace:version:type
```

**Examples**:
- `:rtfs.core:v2.0:intent` - Core RTFS object
- `:com.acme.financial:v1.2:quarterly-analysis-intent` - Custom Intent subtype
- `:org.openai:v1.0:gpt-capability` - Third-party Capability

### 9.2 Version Semantics

- **Major version changes**: Break compatibility
- **Minor version changes**: Add fields (backward compatible)
- **Patch versions**: Fix bugs without schema changes

## 10. Implementation Notes

### 10.1 Serialization

- All objects serialize to/from RTFS native format
- JSON export available for interoperability
- Binary serialization for high-performance scenarios

### 10.2 Validation

- Each object type has a JSON Schema definition
- Runtime validation during parsing
- Optional strict mode for production deployments

### 10.3 Storage Considerations

- **Intents and Capabilities**: Long-lived, indexed storage
- **Plans**: Short-lived active storage, compressed archival
- **Actions**: Append-only immutable log
- **Resources**: Content-addressed with lifecycle management


