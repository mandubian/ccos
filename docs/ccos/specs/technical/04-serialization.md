# RTFS 2.0 Serialization Specification

**Date:** June 23, 2025  
**Version:** 0.1.0-draft  
**Status:** Draft

## Overview

This document defines the canonical serialization formats for RTFS 2.0 objects, supporting multiple output formats while maintaining the homoiconic nature of RTFS. The serialization system provides:

- **Native RTFS Format**: Canonical representation preserving code/data duality
- **JSON Export**: For interoperability with external systems
- **Binary Format**: For high-performance storage and transmission
- **Human-Readable Format**: For debugging and documentation

## Design Principles

1. **Homoiconicity Preservation**: Native format maintains code/data equivalence
2. **Roundtrip Fidelity**: Any object can be serialized and deserialized without loss
3. **Version Compatibility**: Forward/backward compatibility within major versions
4. **Performance Optimization**: Binary format optimized for speed and size
5. **Interoperability**: JSON format for external system integration

## Serialization Formats

### 1. Native RTFS Format (Canonical)

The canonical format preserves the homoiconic nature of RTFS, allowing objects to contain executable code alongside data.

#### Format Specification
- **Encoding**: UTF-8 text
- **Structure**: S-expression based, following RTFS 2.0 grammar
- **Executable Elements**: Functions preserved as executable RTFS code
- **Comments**: Preserved for documentation

#### Intent Object Example
```rtfs
;; RTFS 2.0 Intent Object - Canonical Format
(intent
  :type :rtfs.core:v2.0:intent
  :intent-id "550e8400-e29b-41d4-a716-446655440000"
  :goal "Analyze quarterly sales performance for Q2 2025"
  :created-at "2025-06-23T10:30:00Z"
  :created-by "user:alice@company.com"
  :priority :high
  :constraints {
    :max-cost 25.00
    :deadline "2025-06-25T17:00:00Z"
    :data-locality [:US :EU]
    :security-clearance :confidential
    :preferred-style :executive-formal
  }
  :success-criteria (fn [result] 
    (and (contains? result :executive-summary)
         (contains? result :key-metrics)
         (> (:confidence result) 0.85)))
  :parent-intent "parent-intent-uuid-9876"
  :child-intents ["child-intent-uuid-1111" "child-intent-uuid-2222"]
  :status :active
  :metadata {
    :department "sales"
    :quarter "Q2-2025" 
    :stakeholders ["ceo@company.com" "cfo@company.com"]
  })
```

#### Plan Object Example
```rtfs
;; RTFS 2.0 Plan Object - Canonical Format
(plan
  :type :rtfs.core:v2.0:plan
  :plan-id "plan-67890-abcd-1234-efgh-567890123456"
  :created-at "2025-06-23T10:35:00Z"
  :created-by :arbiter
  :intent-ids ["550e8400-e29b-41d4-a716-446655440000"]
  :strategy :parallel-analysis
  :estimated-cost 18.50
  :estimated-duration 1800
  :program {
    :steps [
      {
        :step-id "step-1"
        :action :fetch-data
        :capability :com.acme.db:v1.0:sales-query
        :params {
          :query "SELECT * FROM sales WHERE quarter = 'Q2-2025'"
          :format :csv
        }
        :expected-output :resource
        :timeout 30
      }
      {
        :step-id "step-2"
        :action :analyze-data
        :capability :com.openai:v1.0:data-analysis
        :depends-on ["step-1"]
        :params {
          :data (resource:ref "step-1.output")
          :analysis-type :quarterly-summary
          :output-format :executive-brief
        }
        :expected-output :document
        :timeout 300
      }
    ]
  }
  :status :ready
  :execution-context {
    :arbiter-reasoning "Chose parallel analysis strategy due to tight deadline constraint"
    :alternative-strategies [:sequential-deep-dive :ai-only-analysis]
    :risk-assessment :low
  })
```

#### Resource Reference Example
```rtfs
;; Resource references preserve semantic meaning
(plan
  :type :rtfs.core:v2.0:plan
  :plan-id "plan-with-resources"
  :program {
    :steps [
      {
        :step-id "process-data"
        :params {
          :input-data (resource:ref "uploaded-file.csv")
          :config-file (resource:handle "resource://configs/analysis-config.json")
        }
      }
    ]
  })
```

### 2. JSON Export Format

For interoperability with external systems that don't support RTFS syntax.

#### Serialization Rules
- **Functions**: Serialized as objects with `type: "function"` and string `body`
- **Keywords**: Converted to strings with preserved `:` prefix
- **Symbols**: Converted to strings
- **Resource References**: Converted to structured objects

#### Intent Object JSON Example
```json
{
  "type": ":rtfs.core:v2.0:intent",
  "intent-id": "550e8400-e29b-41d4-a716-446655440000",
  "goal": "Analyze quarterly sales performance for Q2 2025",
  "created-at": "2025-06-23T10:30:00Z",
  "created-by": "user:alice@company.com",
  "priority": ":high",
  "constraints": {
    "max-cost": 25.00,
    "deadline": "2025-06-25T17:00:00Z",
    "data-locality": [":US", ":EU"],
    "security-clearance": ":confidential",
    "preferred-style": ":executive-formal"
  },
  "success-criteria": {
    "type": "function",
    "params": ["result"],
    "body": "(and (contains? result :executive-summary) (contains? result :key-metrics) (> (:confidence result) 0.85))"
  },
  "parent-intent": "parent-intent-uuid-9876",
  "child-intents": ["child-intent-uuid-1111", "child-intent-uuid-2222"],
  "status": ":active",
  "metadata": {
    "department": "sales",
    "quarter": "Q2-2025",
    "stakeholders": ["ceo@company.com", "cfo@company.com"]
  }
}
```

#### Resource Reference JSON Example
```json
{
  "type": ":rtfs.core:v2.0:plan",
  "plan-id": "plan-with-resources",
  "program": {
    "steps": [
      {
        "step-id": "process-data",
        "params": {
          "input-data": {
            "type": "resource-ref",
            "reference": "uploaded-file.csv"
          },
          "config-file": {
            "type": "resource-handle", 
            "handle": "resource://configs/analysis-config.json"
          }
        }
      }
    ]
  }
}
```

### 3. Binary Format (High-Performance)

Optimized binary format for high-throughput scenarios and efficient storage.

#### Format Specifications
- **Encoding**: Custom binary protocol with type tags
- **Compression**: LZ4 compression for large objects
- **Type System**: Efficient type encoding with lookup tables
- **String Interning**: Deduplicated strings for space efficiency

#### Binary Format Structure
```
[Header: 8 bytes]
[Magic: "RTFS"]     4 bytes
[Version: Major.Minor] 2 bytes  
[Flags: Compressed/etc] 2 bytes

[Type Table: Variable]
[Type Count: uint16]
[Type Entries: Variable length strings]

[String Table: Variable] 
[String Count: uint16]
[String Entries: Length-prefixed UTF-8]

[Object Data: Variable]
[Object Type: uint8 (index into type table)]
[Field Count: uint16]
[Fields: Variable length key-value pairs]
```

#### Type Encoding
```
Object Types:
0x01 = Intent
0x02 = Plan  
0x03 = Action
0x04 = Capability
0x05 = Resource

Value Types:
0x10 = String (index into string table)
0x11 = Integer (varint encoded)
0x12 = Float (IEEE 754 double)
0x13 = Boolean (single byte)
0x14 = Keyword (string table index with flag)
0x15 = Vector (count + elements)
0x16 = Map (count + key-value pairs)
0x17 = Function (string table index for body)
0x18 = Resource Reference (string table index)
```

### 4. Human-Readable Format (Debug/Documentation)

Enhanced format with formatting, comments, and metadata for human consumption.

#### Intent Object Debug Format
```rtfs
;; ============================================================================
;; RTFS 2.0 Intent Object
;; ID: 550e8400-e29b-41d4-a716-446655440000
;; Created: 2025-06-23T10:30:00Z by user:alice@company.com
;; Status: ACTIVE | Priority: HIGH
;; ============================================================================

(intent
  ;; === Core Identity ===
  :type         :rtfs.core:v2.0:intent
  :intent-id    "550e8400-e29b-41d4-a716-446655440000"
  :created-at   "2025-06-23T10:30:00Z"
  :created-by   "user:alice@company.com"
  :status       :active
  
  ;; === Goal Definition ===
  :goal         "Analyze quarterly sales performance for Q2 2025"
  :priority     :high
  
  ;; === Execution Constraints ===
  :constraints  {
    :max-cost           25.00          ; Maximum budget: $25.00
    :deadline           "2025-06-25T17:00:00Z"  ; Hard deadline: Friday 5PM
    :data-locality      [:US :EU]      ; GDPR compliance: US/EU only
    :security-clearance :confidential  ; Requires confidential clearance
    :preferred-style    :executive-formal ; Executive summary format
  }
  
  ;; === Success Validation Function ===
  :success-criteria (fn [result] 
    ;; Validates that result contains required fields with sufficient confidence
    (and (contains? result :executive-summary)
         (contains? result :key-metrics)
         (> (:confidence result) 0.85)))
  
  ;; === Intent Graph Relationships ===
  :parent-intent    "parent-intent-uuid-9876"     ; Part of larger goal
  :child-intents    ["child-intent-uuid-1111"     ; Sub-goals spawned
                     "child-intent-uuid-2222"]
  
  ;; === Additional Context ===
  :metadata {
    :department     "sales"
    :quarter        "Q2-2025"
    :stakeholders   ["ceo@company.com" "cfo@company.com"]
  })

;; ============================================================================
;; Intent Graph Context:
;;   └─ Parent: parent-intent-uuid-9876 (Strategic planning)
;;       ├─ Current: 550e8400-e29b-41d4-a716-446655440000 (Sales analysis)  
;;       ├─ Sibling: child-intent-uuid-1111 (Marketing analysis)
;;       └─ Sibling: child-intent-uuid-2222 (Revenue forecasting)
;; ============================================================================
```

## Serialization API

### Rust Implementation Structure

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

pub trait RtfsSerializable {
    /// Serialize to canonical RTFS format
    fn to_rtfs(&self) -> Result<String, SerializationError>;
    
    /// Serialize to JSON format
    fn to_json(&self) -> Result<String, SerializationError>;
    
    /// Serialize to binary format
    fn to_binary(&self) -> Result<Vec<u8>, SerializationError>;
    
    /// Serialize to human-readable debug format
    fn to_debug(&self) -> Result<String, SerializationError>;
    
    /// Deserialize from any supported format (auto-detect)
    fn from_bytes(data: &[u8]) -> Result<Self, SerializationError> 
    where Self: Sized;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntentObject {
    pub r#type: String,
    pub intent_id: String,
    pub goal: String,
    // ... other fields
}

impl RtfsSerializable for IntentObject {
    fn to_rtfs(&self) -> Result<String, SerializationError> {
        // Implementation converts to S-expression format
        Ok(format!(
            "(intent\n  :type {}\n  :intent-id \"{}\"\n  :goal \"{}\"\n  ...)",
            self.r#type, self.intent_id, self.goal
        ))
    }
    
    // ... other implementations
}
```

### Format Detection
```rust
pub enum SerializationFormat {
    Rtfs,    // S-expression format
    Json,    // JSON format  
    Binary,  // Custom binary format
    Auto,    // Auto-detect from content
}

pub fn detect_format(data: &[u8]) -> SerializationFormat {
    if data.starts_with(b"RTFS") {
        SerializationFormat::Binary
    } else if data.starts_with(b"(") {
        SerializationFormat::Rtfs
    } else if data.starts_with(b"{") {
        SerializationFormat::Json
    } else {
        SerializationFormat::Auto
    }
}
```

## Performance Characteristics

### Format Comparison
| Format | Size | Parse Speed | Human Readable | Preserves Code |
|--------|------|-------------|----------------|----------------|
| Native RTFS | 100% | Fast | ✅ Yes | ✅ Yes |
| JSON | 120% | Fast | ✅ Yes | ⚠️ Limited |
| Binary | 60% | Very Fast | ❌ No | ❌ No |
| Debug | 150% | Slow | ✅ Enhanced | ✅ Yes |

### Benchmarks (Estimated)
- **Native RTFS**: 1x baseline (parse: 10ms, serialize: 5ms)
- **JSON**: 1.2x size, 0.8x speed (parse: 8ms, serialize: 6ms)
- **Binary**: 0.6x size, 3x speed (parse: 3ms, serialize: 2ms)
- **Debug**: 1.5x size, 0.5x speed (parse: 20ms, serialize: 15ms)

## Interoperability

### MIME Types
- `application/rtfs` - Native RTFS format
- `application/rtfs+json` - JSON export format  
- `application/rtfs+binary` - Binary format
- `text/rtfs+debug` - Human-readable debug format

### HTTP Headers
```http
Content-Type: application/rtfs
X-RTFS-Version: 2.0
X-RTFS-Object-Type: intent
X-RTFS-Schema: https://rtfs.ai/schemas/v2.0/intent.json
```

### File Extensions
- `.rtfs` - Native RTFS format
- `.rtfs.json` - JSON export
- `.rtfs.bin` - Binary format  
- `.rtfs.debug` - Debug format

## Validation Integration

### Schema-Driven Serialization
```rust
impl IntentObject {
    pub fn validate_and_serialize(&self, format: SerializationFormat) -> Result<Vec<u8>, Error> {
        // 1. Validate against JSON schema
        self.validate()?;
        
        // 2. Serialize to requested format
        match format {
            SerializationFormat::Rtfs => Ok(self.to_rtfs()?.into_bytes()),
            SerializationFormat::Json => Ok(self.to_json()?.into_bytes()),
            SerializationFormat::Binary => self.to_binary(),
            SerializationFormat::Auto => self.to_rtfs().map(|s| s.into_bytes()),
        }
    }
}
```

## Migration Strategy

### Phase 1: Native RTFS Support (Week 6)
1. Implement S-expression serialization for all 5 object types
2. Add roundtrip testing (serialize → deserialize → compare)
3. Integrate with existing RTFS parser

### Phase 2: JSON Export (Week 7)  
1. Implement JSON serialization with function preservation
2. Add JSON Schema validation
3. Test interoperability with external systems

### Phase 3: Binary Format (Week 8)
1. Design and implement binary protocol
2. Add compression support
3. Performance benchmarking and optimization

### Phase 4: Tooling Integration (Week 9)
1. Add format detection to REPL
2. Implement conversion utilities
3. Create validation CLI tools

## Error Handling

### Serialization Errors
```rust
#[derive(Debug, thiserror::Error)]
pub enum SerializationError {
    #[error("Invalid object type: {0}")]
    InvalidObjectType(String),
    
    #[error("Missing required field: {0}")]
    MissingField(String),
    
    #[error("Function serialization failed: {0}")]
    FunctionSerializationError(String),
    
    #[error("Binary format version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: String, actual: String },
    
    #[error("Compression error: {0}")]
    CompressionError(String),
}
```

### Recovery Strategies
1. **Graceful Degradation**: Fall back to simpler formats on error
2. **Partial Serialization**: Serialize valid fields, mark invalid ones  
3. **Error Context**: Provide detailed error location information
4. **Format Migration**: Automatic upgrade from older format versions

## Testing Strategy

### Roundtrip Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_intent_roundtrip_rtfs() {
        let original = create_sample_intent();
        let serialized = original.to_rtfs().unwrap();
        let deserialized = IntentObject::from_rtfs(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }
    
    #[test]
    fn test_all_formats_consistency() {
        let intent = create_sample_intent();
        
        let rtfs_json = intent.to_json().unwrap();
        let json_intent = IntentObject::from_json(&rtfs_json).unwrap();
        
        let binary_data = intent.to_binary().unwrap();
        let binary_intent = IntentObject::from_binary(&binary_data).unwrap();
        
        assert_eq!(intent, json_intent);
        assert_eq!(intent, binary_intent);
    }
}
```

### Integration Tests
1. **Cross-Format Validation**: Ensure all formats produce equivalent objects
2. **Schema Compliance**: Validate all serialized objects against JSON schemas
3. **Performance Testing**: Benchmark serialization/deserialization performance
4. **Interoperability Testing**: Validate JSON format with external tools

## Future Enhancements

1. **Streaming Support**: Support for large objects via streaming serialization
2. **Delta Serialization**: Incremental updates for large, frequently-modified objects  
3. **Compression Options**: Multiple compression algorithms (LZ4, Zstd, Brotli)
4. **Encryption Support**: Built-in encryption for sensitive objects
5. **Schema Evolution**: Automatic migration between schema versions
