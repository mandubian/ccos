# RTFS 2.0 Core Objects Specification

**Date:** June 23, 2025  
**Version:** 0.1.0-draft  
**Status:** Draft

## Overview

RTFS 2.0 introduces five first-class object types that replace the monolithic `Task` object from RTFS 1.0. These objects are designed to support the Cognitive Computing Operating System (CCOS) architecture with clear separation of concerns, formal namespacing, and rich metadata support.

## The Five Core Objects

### 1. Intent Object

**Purpose**: Represents a user's goal or desired outcome in the Living Intent Graph.

**Structure**:
```rtfs
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

**Key Properties**:
- **Persistent**: Lives in the Intent Graph until explicitly archived
- **Hierarchical**: Can have parent/child relationships
- **Executable Logic**: `success-criteria` can contain executable functions
- **Rich Constraints**: Multiple constraint types for Arbiter decision-making

### 2. Plan Object

**Purpose**: A concrete, executable RTFS program generated to fulfill one or more Intents.

**Structure**:
```rtfs
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

**Key Properties**:
- **Transient but Archivable**: Executed once, then archived for audit
- **Executable Script**: Contains a full RTFS program, allowing for control flow, variables, and complex logic.
- **Metadata Rich**: Includes cost estimates, reasoning, alternatives
- **Implicit Dependencies**: Data dependencies are handled naturally by variable scope (`let`).

### 3. Action Object

**Purpose**: An immutable record of a single executed operation, forming the Causal Chain.

**Structure**:
```rtfs
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

**Key Properties**:
- **Immutable**: Never modified after creation
- **Cryptographically Signed**: Ensures audit trail integrity
- **Complete Provenance**: Links back to Intent, Plan, and Capability
- **Performance Metrics**: Duration, cost, success status

### 4. Capability Object

**Purpose**: Formal declaration of a service or function available in the Global Function Mesh.

**Structure**:
```rtfs
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

**Key Properties**:
- **Rich Metadata**: Complete SLA, technical specs, compliance info
- **Discoverable**: Tags and categories for marketplace search
- **Verifiable**: Reputation, certifications, examples
- **Economic Info**: Pricing, rate limits, geographic restrictions

### 5. Resource Object

**Purpose**: Handle or reference to large data payloads, keeping other objects lightweight.

**Structure**:
```rtfs
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

**Key Properties**:
- **Location Agnostic**: Can reference local files, cloud storage, databases
- **Lifecycle Managed**: Automatic cleanup and archival
- **Access Controlled**: Permissions and expiration
- **Schema Aware**: Rich metadata about content structure

## Object Relationships

### Dependency Graph
```
Intent (persistent)
  ↓ generates
Plan (transient → archived)  
  ↓ executes via
Action (immutable) ← uses → Capability (persistent)
  ↓ references
Resource (managed lifecycle)
```

### Key Relationships
- **Intent → Plan**: 1:many (one Intent can have multiple Plans over time)
- **Plan → Action**: 1:many (one Plan generates multiple Actions during execution)
- **Action → Resource**: many:many (Actions can create/read multiple Resources)
- **Capability → Action**: 1:many (one Capability can be used by multiple Actions)

## Namespacing and Versioning

All objects use the formal type system:
```
:namespace:version:type
```

**Examples**:
- `:rtfs.core:v2.0:intent` - Core RTFS object
- `:com.acme.financial:v1.2:quarterly-analysis-intent` - Custom Intent subtype
- `:org.openai:v1.0:gpt-capability` - Third-party Capability

**Version Semantics**:
- Major version changes break compatibility
- Minor version changes add fields (backward compatible)
- Patch versions fix bugs without schema changes

## Implementation Notes

### Serialization
- All objects serialize to/from RTFS native format
- JSON export available for interoperability
- Binary serialization for high-performance scenarios

### Validation
- Each object type has a JSON Schema definition
- Runtime validation during parsing
- Optional strict mode for production deployments

### Storage Considerations
- Intents and Capabilities: Long-lived, indexed storage
- Plans: Short-lived active storage, compressed archival
- Actions: Append-only immutable log
- Resources: Content-addressed with lifecycle management

---

## Next Steps

1. **Schema Definition**: Create JSON Schema files for each object type
2. **Parser Extension**: Extend RTFS parser to handle new object syntax
3. **Validation**: Implement runtime validation for all object types
4. **Examples**: Create comprehensive example library
5. **Testing**: Build test suite covering all object interactions
