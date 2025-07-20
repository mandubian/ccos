# RTFS 2.0 Object Schemas

**Date:** June 23, 2025  
**Version:** 0.1.0-draft  
**Status:** Draft

## Overview

This document provides JSON Schema definitions for all RTFS 2.0 core objects. These schemas enable:
- Runtime validation during parsing
- IDE support with autocomplete and error checking
- API contract validation
- Documentation generation
- Cross-language interoperability

## Schema Design Principles

1. **Strict by Default**: All schemas enforce required fields and types
2. **Extensible**: Support for additional properties via `metadata` fields
3. **Versioned**: Each schema includes version information
4. **Self-Describing**: Rich descriptions and examples
5. **Validation Ready**: Suitable for runtime validation

## Common Schema Components

### Base Object Schema
```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "definitions": {
    "rtfs_uuid": {
      "type": "string",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "description": "UUID v4 format identifier"
    },
    "rtfs_timestamp": {
      "type": "string",
      "pattern": "^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(\\.\\d{3})?Z$",
      "description": "ISO 8601 timestamp in UTC"
    },
    "rtfs_versioned_type": {
      "type": "string",
      "pattern": "^:[a-zA-Z][a-zA-Z0-9._-]*:v\\d+(\\.\\d+)*:[a-zA-Z][a-zA-Z0-9_-]*$",
      "description": "Versioned type identifier: :namespace:version:type"
    },
    "rtfs_resource_handle": {
      "type": "string",
      "pattern": "^resource://.*$",
      "description": "Resource handle URI"
    },
    "rtfs_money": {
      "type": "number",
      "minimum": 0,
      "multipleOf": 0.01,
      "description": "Money amount with cent precision"
    }
  }
}
```

## 1. Intent Object Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://rtfs.ai/schemas/v2.0/intent.json",
  "title": "RTFS 2.0 Intent Object",
  "description": "Represents a user's goal or desired outcome in the Living Intent Graph",
  "type": "object",
  "required": [
    "type",
    "intent-id", 
    "goal",
    "created-at",
    "created-by",
    "status"
  ],
  "properties": {
    "type": {
      "$ref": "#/definitions/rtfs_versioned_type",
      "const": ":rtfs.core:v2.0:intent",
      "description": "Object type identifier"
    },
    "intent-id": {
      "$ref": "#/definitions/rtfs_uuid",
      "description": "Unique identifier for this intent"
    },
    "goal": {
      "type": "string",
      "minLength": 10,
      "maxLength": 1000,
      "description": "Human-readable description of the desired outcome"
    },
    "created-at": {
      "$ref": "#/definitions/rtfs_timestamp",
      "description": "When this intent was created"
    },
    "created-by": {
      "type": "string",
      "description": "Identity of the creator (user, system, agent)"
    },
    "priority": {
      "type": "string",
      "enum": ["low", "normal", "high", "urgent", "critical"],
      "default": "normal",
      "description": "Execution priority level"
    },
    "constraints": {
      "type": "object",
      "properties": {
        "max-cost": {
          "$ref": "#/definitions/rtfs_money",
          "description": "Maximum cost willing to spend"
        },
        "deadline": {
          "$ref": "#/definitions/rtfs_timestamp",
          "description": "Hard deadline for completion"
        },
        "data-locality": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[A-Z]{2}$"
          },
          "description": "Allowed geographic regions (ISO country codes)"
        },
        "security-clearance": {
          "type": "string",
          "enum": ["public", "internal", "confidential", "secret", "top-secret"],
          "description": "Required security clearance level"
        },
        "preferred-style": {
          "type": "string",
          "enum": ["casual", "formal", "technical", "executive", "creative"],
          "description": "Preferred communication/output style"
        }
      },
      "additionalProperties": true,
      "description": "Execution constraints and preferences"
    },
    "success-criteria": {
      "type": "object",
      "properties": {
        "type": {
          "const": "function"
        },
        "params": {
          "type": "array",
          "items": {"type": "string"}
        },
        "body": {
          "type": "string"
        }
      },
      "description": "Executable function to validate success (RTFS syntax)"
    },
    "parent-intent": {
      "$ref": "#/definitions/rtfs_uuid",
      "description": "Parent intent ID if this is a sub-goal"
    },
    "child-intents": {
      "type": "array",
      "items": {
        "$ref": "#/definitions/rtfs_uuid"
      },
      "description": "List of child intent IDs"
    },
    "status": {
      "type": "string",
      "enum": ["draft", "active", "paused", "completed", "failed", "archived"],
      "description": "Current status of the intent"
    },
    "metadata": {
      "type": "object",
      "additionalProperties": true,
      "description": "Additional application-specific metadata"
    }
  },
  "additionalProperties": false
}
```

## 2. Plan Object Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://rtfs.ai/schemas/v2.0/plan.json",
  "title": "RTFS 2.0 Plan Object",
  "description": "A concrete, executable RTFS program generated to fulfill one or more Intents",
  "type": "object",
  "required": [
    "type",
    "plan-id",
    "created-at",
    "created-by",
    "intent-ids",
    "program",
    "status"
  ],
  "properties": {
    "type": {
      "$ref": "#/definitions/rtfs_versioned_type",
      "const": ":rtfs.core:v2.0:plan",
      "description": "Object type identifier"
    },
    "plan-id": {
      "$ref": "#/definitions/rtfs_uuid",
      "description": "Unique identifier for this plan"
    },
    "created-at": {
      "$ref": "#/definitions/rtfs_timestamp",
      "description": "When this plan was generated"
    },
    "created-by": {
      "type": "string",
      "enum": ["arbiter", "user", "agent"],
      "description": "Who or what created this plan"
    },
    "intent-ids": {
      "type": "array",
      "items": {
        "$ref": "#/definitions/rtfs_uuid"
      },
      "minItems": 1,
      "description": "List of intent IDs this plan addresses"
    },
    "strategy": {
      "type": "string",
      "enum": [
        "sequential",
        "parallel",
        "hybrid",
        "cost-optimized", 
        "speed-optimized",
        "reliability-optimized"
      ],
      "description": "High-level execution strategy"
    },
    "estimated-cost": {
      "$ref": "#/definitions/rtfs_money",
      "description": "Estimated total execution cost"
    },
    "estimated-duration": {
      "type": "integer",
      "minimum": 1,
      "description": "Estimated execution time in seconds"
    },
    "program": {
      "type": "object",
      "required": ["steps"],
      "properties": {
        "steps": {
          "type": "array",
          "items": {
            "$ref": "#/definitions/plan_step"
          },
          "minItems": 1,
          "description": "Ordered list of execution steps"
        }
      },
      "description": "Executable program structure"
    },
    "status": {
      "type": "string",
      "enum": ["draft", "ready", "executing", "completed", "failed", "cancelled"],
      "description": "Current execution status"
    },
    "execution-context": {
      "type": "object",
      "properties": {
        "arbiter-reasoning": {
          "type": "string",
          "description": "Arbiter's reasoning for this plan"
        },
        "alternative-strategies": {
          "type": "array",
          "items": {"type": "string"},
          "description": "Other strategies considered"
        },
        "risk-assessment": {
          "type": "string",
          "enum": ["low", "medium", "high", "critical"],
          "description": "Overall risk level assessment"
        }
      },
      "description": "Additional execution context and metadata"
    }
  },
  "definitions": {
    "plan_step": {
      "type": "object",
      "required": ["step-id", "action", "capability", "inputs", "outputs"],
      "properties": {
        "step-id": {
          "type": "string",
          "pattern": "^step-[a-zA-Z0-9-]+$",
          "description": "Unique step identifier within the plan"
        },
        "action": {
          "type": "string",
          "enum": [
            "fetch-data", "analyze-data", "transform-data",
            "generate-content", "send-notification", "store-result",
            "validate-result", "aggregate-results"
          ],
          "description": "Type of action to perform"
        },
        "capability": {
          "$ref": "#/definitions/rtfs_versioned_type",
          "description": "Capability to use for this step"
        },
        "depends-on": {
          "type": "array",
          "items": {"type": "string"},
          "description": "Step IDs this step depends on"
        },
        "inputs": {
          "type": "object",
          "description": "Input parameters to pass to the capability. Must conform to the capability's input schema."
        },
        "outputs": {
          "type": "object",
          "required": ["schema", "binding"],
          "properties": {
            "schema": {
              "type": "object",
              "description": "A JSON Schema describing the structure of the output data. This must match the output schema of the selected capability."
            },
            "binding": {
              "type": "string",
              "pattern": "^[a-zA-Z_][a-zA-Z0-9_]*$",
              "description": "A variable name to which the output of this step will be bound, making it available for subsequent steps."
            }
          },
          "description": "Defines the expected outputs and how they are bound to variables for subsequent steps."
        },
        "timeout": {
          "type": "integer",
          "minimum": 1,
          "description": "Step timeout in seconds"
        }
      }
    }
  },
  "additionalProperties": false
}

## 3. Action Object Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://rtfs.ai/schemas/v2.0/action.json",
  "title": "RTFS 2.0 Action Object",
  "description": "An immutable record of a single executed operation in the Causal Chain",
  "type": "object",
  "required": [
    "type",
    "action-id",
    "timestamp",
    "plan-id",
    "step-id",
    "intent-id",
    "capability-used",
    "executor",
    "input",
    "output",
    "execution",
    "signature"
  ],
  "properties": {
    "type": {
      "$ref": "#/definitions/rtfs_versioned_type",
      "const": ":rtfs.core:v2.0:action",
      "description": "Object type identifier"
    },
    "action-id": {
      "$ref": "#/definitions/rtfs_uuid",
      "description": "Unique identifier for this action"
    },
    "timestamp": {
      "$ref": "#/definitions/rtfs_timestamp",
      "description": "Exact time when action was executed"
    },
    "plan-id": {
      "$ref": "#/definitions/rtfs_uuid",
      "description": "Plan that contained this action"
    },
    "step-id": {
      "type": "string",
      "description": "Step identifier within the plan"
    },
    "intent-id": {
      "$ref": "#/definitions/rtfs_uuid",
      "description": "Ultimate intent this action serves"
    },
    "capability-used": {
      "$ref": "#/definitions/rtfs_versioned_type",
      "description": "Capability that was invoked"
    },
    "executor": {
      "type": "object",
      "required": ["type", "id"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["local", "agent", "service", "arbiter"],
          "description": "Type of executor"
        },
        "id": {
          "type": "string",
          "description": "Unique executor identifier"
        },
        "node": {
          "type": "string",
          "description": "Network node where execution occurred"
        },
        "version": {
          "type": "string",
          "description": "Executor version"
        }
      },
      "description": "Information about what executed this action"
    },
    "input": {
      "type": "object",
      "description": "Input parameters provided to the capability"
    },
    "output": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["resource", "data", "error", "notification"],
          "description": "Type of output produced"
        },
        "handle": {
          "$ref": "#/definitions/rtfs_resource_handle",
          "description": "Resource handle if output is a resource"
        },
        "data": {
          "description": "Direct data if output is small"
        },
        "size": {
          "type": "integer",
          "minimum": 0,
          "description": "Size in bytes"
        },
        "checksum": {
          "type": "string",
          "pattern": "^(sha256|md5):[a-f0-9]+$",
          "description": "Content checksum for verification"
        },
        "metadata": {
          "type": "object",
          "description": "Additional output metadata"
        }
      },
      "description": "Action output information"
    },
    "execution": {
      "type": "object",
      "required": ["started-at", "completed-at", "status"],
      "properties": {
        "started-at": {
          "$ref": "#/definitions/rtfs_timestamp",
          "description": "Execution start time"
        },
        "completed-at": {
          "$ref": "#/definitions/rtfs_timestamp",
          "description": "Execution completion time"
        },
        "duration": {
          "type": "number",
          "minimum": 0,
          "description": "Execution duration in seconds"
        },
        "cost": {
          "$ref": "#/definitions/rtfs_money",
          "description": "Actual execution cost"
        },
        "status": {
          "type": "string",
          "enum": ["success", "failure", "timeout", "cancelled"],
          "description": "Execution outcome"
        },
        "error": {
          "type": "object",
          "properties": {
            "code": {"type": "string"},
            "message": {"type": "string"},
            "details": {"type": "object"}
          },
          "description": "Error information if status is failure"
        }
      },
      "description": "Execution timing and outcome"
    },
    "signature": {
      "type": "object",
      "required": ["signed-by", "signature", "algorithm"],
      "properties": {
        "signed-by": {
          "type": "string",
          "description": "Cryptographic key identifier"
        },
        "signature": {
          "type": "string",
          "description": "Cryptographic signature"
        },
        "algorithm": {
          "type": "string",
          "enum": ["ed25519", "ecdsa-p256", "rsa-2048"],
          "description": "Signature algorithm used"
        }
      },
      "description": "Cryptographic signature for audit trail integrity"
    }
  },
  "additionalProperties": false
}
```

## 4. Capability Object Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://rtfs.ai/schemas/v2.0/capability.json",
  "title": "RTFS 2.0 Capability Object",
  "description": "Formal declaration of a service or function available in the Global Function Mesh",
  "type": "object",
  "required": [
    "type",
    "capability-id",
    "created-at",
    "provider",
    "function",
    "sla",
    "status"
  ],
  "properties": {
    "type": {
      "$ref": "#/definitions/rtfs_versioned_type",
      "const": ":rtfs.core:v2.0:capability",
      "description": "Object type identifier"
    },
    "capability-id": {
      "$ref": "#/definitions/rtfs_versioned_type",
      "description": "Unique capability identifier with versioning"
    },
    "created-at": {
      "$ref": "#/definitions/rtfs_timestamp",
      "description": "When this capability was registered"
    },
    "provider": {
      "type": "object",
      "required": ["name", "contact"],
      "properties": {
        "name": {
          "type": "string",
          "minLength": 1,
          "maxLength": 100,
          "description": "Provider organization name"
        },
        "contact": {
          "type": "string",
          "format": "email",
          "description": "Provider contact email"
        },
        "node-id": {
          "type": "string",
          "description": "Network node identifier"
        },
        "reputation": {
          "type": "number",
          "minimum": 0,
          "maximum": 5,
          "description": "Provider reputation score (0-5)"
        },
        "certifications": {
          "type": "array",
          "items": {
            "type": "string",
            "enum": ["iso-27001", "soc2-type1", "soc2-type2", "hipaa", "gdpr-compliant"]
          },
          "description": "Security and compliance certifications"
        }
      },
      "description": "Capability provider information"
    },
    "function": {
      "type": "object",
      "required": ["name", "description", "signature"],
      "properties": {
        "name": {
          "type": "string",
          "pattern": "^[a-zA-Z][a-zA-Z0-9_-]*$",
          "description": "Function name"
        },
        "description": {
          "type": "string",
          "minLength": 10,
          "maxLength": 500,
          "description": "Human-readable function description"
        },
        "signature": {
          "type": "object",
          "required": ["inputs", "outputs"],
          "properties": {
            "inputs": {
              "type": "object",
              "patternProperties": {
                "^[a-zA-Z][a-zA-Z0-9_-]*$": {
                  "oneOf": [
                    {"type": "array", "items": {"type": "string"}},
                    {"type": "string"}
                  ]
                }
              },
              "description": "Input parameter specifications"
            },
            "outputs": {
              "type": "object",
              "description": "Output specifications"
            }
          },
          "description": "Function signature with input/output types"
        },
        "examples": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["input", "output"],
            "properties": {
              "input": {"type": "object"},
              "output": {"type": "object"},
              "description": {"type": "string"}
            }
          },
          "description": "Usage examples"
        }
      },
      "description": "Function specification"
    },
    "sla": {
      "type": "object",
      "required": ["availability"],
      "properties": {
        "cost-per-call": {
          "$ref": "#/definitions/rtfs_money",
          "description": "Cost per function invocation"
        },
        "max-response-time": {
          "type": "number",
          "minimum": 0.1,
          "description": "Maximum response time in seconds"
        },
        "availability": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "description": "Availability percentage (0.0-1.0)"
        },
        "rate-limit": {
          "type": "object",
          "properties": {
            "calls": {"type": "integer", "minimum": 1},
            "period": {"type": "string", "enum": ["second", "minute", "hour", "day"]}
          },
          "description": "Rate limiting configuration"
        },
        "data-retention": {
          "type": "integer",
          "minimum": 0,
          "description": "Data retention period in days"
        },
        "geographic-restrictions": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^[A-Z]{2}$"
          },
          "description": "Allowed geographic regions (ISO country codes)"
        }
      },
      "description": "Service Level Agreement"
    },
    "technical": {
      "type": "object",
      "properties": {
        "runtime": {
          "type": "string",
          "description": "Runtime environment"
        },
        "version": {
          "type": "string",
          "description": "Runtime version"
        },
        "security": {
          "type": "array",
          "items": {"type": "string"},
          "description": "Security features implemented"
        },
        "compliance": {
          "type": "array",
          "items": {
            "type": "string",
            "enum": ["gdpr", "ccpa", "hipaa", "pci-dss", "sox"]
          },
          "description": "Compliance standards met"
        }
      },
      "description": "Technical implementation details"
    },
    "status": {
      "type": "string",
      "enum": ["draft", "active", "deprecated", "disabled"],
      "description": "Current capability status"
    },
    "marketplace": {
      "type": "object",
      "properties": {
        "listed": {
          "type": "boolean",
          "description": "Whether capability is publicly listed"
        },
        "featured": {
          "type": "boolean",
          "description": "Whether capability is featured"
        },
        "tags": {
          "type": "array",
          "items": {"type": "string"},
          "description": "Search tags"
        },
        "category": {
          "type": "string",
          "enum": [
            "data-access", "data-processing", "ml-inference",
            "content-generation", "communication", "storage",
            "authentication", "monitoring", "other"
          ],
          "description": "Marketplace category"
        }
      },
      "description": "Marketplace listing information"
    }
  },
  "additionalProperties": false
}
```

## 5. Resource Object Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://rtfs.ai/schemas/v2.0/resource.json",
  "title": "RTFS 2.0 Resource Object",
  "description": "Handle or reference to large data payloads with lifecycle management",
  "type": "object",
  "required": [
    "type",
    "resource-id",
    "handle",
    "created-at",
    "created-by",
    "content",
    "storage",
    "lifecycle"
  ],
  "properties": {
    "type": {
      "$ref": "#/definitions/rtfs_versioned_type",
      "const": ":rtfs.core:v2.0:resource",
      "description": "Object type identifier"
    },
    "resource-id": {
      "$ref": "#/definitions/rtfs_uuid",
      "description": "Unique resource identifier"
    },
    "handle": {
      "$ref": "#/definitions/rtfs_resource_handle",
      "description": "URI handle for accessing the resource"
    },
    "created-at": {
      "$ref": "#/definitions/rtfs_timestamp",
      "description": "Resource creation timestamp"
    },
    "created-by": {
      "type": "string",
      "description": "Action or process that created this resource"
    },
    "content": {
      "type": "object",
      "required": ["type", "size"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["file", "stream", "database", "api-endpoint", "memory"],
          "description": "Type of content"
        },
        "mime-type": {
          "type": "string",
          "description": "MIME type of the content"
        },
        "size": {
          "type": "integer",
          "minimum": 0,
          "description": "Content size in bytes"
        },
        "encoding": {
          "type": "string",
          "enum": ["utf-8", "utf-16", "ascii", "binary"],
          "description": "Content encoding"
        },
        "checksum": {
          "type": "object",
          "required": ["algorithm", "value"],
          "properties": {
            "algorithm": {
              "type": "string",
              "enum": ["sha256", "sha512", "md5"],
              "description": "Checksum algorithm"
            },
            "value": {
              "type": "string",
              "description": "Checksum value"
            }
          },
          "description": "Content integrity checksum"
        }
      },
      "description": "Content metadata"
    },
    "storage": {
      "type": "object",
      "required": ["backend"],
      "properties": {
        "backend": {
          "type": "string",
          "enum": ["local", "s3", "gcs", "azure", "database", "memory"],
          "description": "Storage backend type"
        },
        "location": {
          "type": "string",
          "description": "Storage location (path, URL, etc.)"
        },
        "region": {
          "type": "string",
          "description": "Geographic region"
        },
        "encryption": {
          "type": "string",
          "enum": ["none", "aes-256", "aes-128", "custom"],
          "description": "Encryption method"
        },
        "access-policy": {
          "type": "string",
          "enum": ["private", "authenticated-read", "public-read", "custom"],
          "description": "Access control policy"
        }
      },
      "description": "Storage configuration"
    },
    "lifecycle": {
      "type": "object",
      "properties": {
        "ttl": {
          "type": "integer",
          "minimum": 0,
          "description": "Time to live in seconds"
        },
        "auto-cleanup": {
          "type": "boolean",
          "description": "Whether to automatically clean up expired resources"
        },
        "archive-after": {
          "type": "integer",
          "minimum": 0,
          "description": "Archive after N seconds"
        },
        "backup-policy": {
          "type": "string",
          "enum": ["none", "daily", "weekly", "custom"],
          "description": "Backup policy"
        }
      },
      "description": "Lifecycle management configuration"
    },
    "metadata": {
      "type": "object",
      "properties": {
        "source": {
          "type": "string",
          "description": "Original source of the data"
        },
        "description": {
          "type": "string",
          "description": "Human-readable description"
        },
        "tags": {
          "type": "array",
          "items": {"type": "string"},
          "description": "Searchable tags"
        },
        "schema": {
          "type": "object",
          "description": "Data schema information"
        }
      },
      "description": "Additional metadata"
    },
    "access": {
      "type": "object",
      "required": ["permissions"],
      "properties": {
        "permissions": {
          "type": "array",
          "items": {
            "type": "string",
            "enum": ["read", "write", "delete", "admin"]
          },
          "description": "Access permissions"
        },
        "expires-at": {
          "$ref": "#/definitions/rtfs_timestamp",
          "description": "Access expiration time"
        },
        "accessed-by": {
          "type": "array",
          "items": {"type": "string"},
          "description": "List of entities that have accessed this resource"
        },
        "access-count": {
          "type": "integer",
          "minimum": 0,
          "description": "Number of times accessed"
        }
      },
      "description": "Access control and tracking"
    }
  },
  "additionalProperties": false
}
```

## Schema Validation Implementation

### Runtime Validation
```rust
// Example Rust validation code structure
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
pub struct IntentObject {
    #[validate(regex = "RTFS_VERSIONED_TYPE_PATTERN")]
    pub r#type: String,
    
    #[validate(regex = "UUID_PATTERN")]
    pub intent_id: String,
    
    #[validate(length(min = 10, max = 1000))]
    pub goal: String,
    
    // ... other fields
}
```

### Validation Rules
1. **Required Fields**: All required fields must be present
2. **Type Checking**: Field types must match schema definitions
3. **Format Validation**: UUIDs, timestamps, emails must be valid
4. **Range Validation**: Numbers must be within specified ranges
5. **Enum Validation**: String fields with enum constraints
6. **Pattern Validation**: Regular expression matching for complex types

## Usage Examples

### Schema-Driven Object Creation
```rtfs
;; RTFS 2.0 with schema validation
(intent
  :type :rtfs.core:v2.0:intent           ;; validates against versioned_type pattern
  :intent-id "550e8400-e29b-41d4-a716-446655440000"  ;; validates as UUID
  :goal "Process quarterly financial reports"         ;; validates length 10-1000
  :priority :high                                    ;; validates against enum
  :created-at "2025-06-23T10:30:00Z"                ;; validates ISO 8601
  :created-by "user:alice@company.com"
  :status :active)
```

### API Integration
```json
{
  "Content-Type": "application/rtfs+json",
  "X-RTFS-Schema": "https://rtfs.ai/schemas/v2.0/intent.json"
}
```

## Next Steps

1. **Implementation**: Integrate schemas into RTFS compiler validation
2. **Testing**: Create comprehensive validation test suite  
3. **Documentation**: Generate API docs from schemas
4. **IDE Support**: Enable schema-based autocomplete and validation
5. **Interoperability**: Support JSON/YAML export with schema validation
