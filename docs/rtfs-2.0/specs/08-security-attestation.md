# RTFS 2.0 Security and Attestation Specification

**Version**: 2.0.0  
**Status**: Stable  
**Date**: July 2025  
**Based on**: Issue #43 Implementation

## 1. Overview

The RTFS 2.0 Security and Attestation System provides comprehensive security features for capability verification, integrity checking, and provenance tracking. It ensures that capabilities are trustworthy, tamper-proof, and traceable throughout their lifecycle.

## 2. Security Architecture

### 2.1 Core Security Components

- **Capability Attestation**: Digital signatures and verification
- **Provenance Tracking**: Chain of custody and source verification
- **Content Hashing**: Integrity verification using cryptographic hashes
- **Schema Validation**: Input/output validation using RTFS native types
- **Permission System**: Fine-grained capability permissions

### 2.2 Security Principles

1. **Zero Trust**: Verify everything, trust nothing by default
2. **Defense in Depth**: Multiple layers of security controls
3. **Principle of Least Privilege**: Minimal required permissions
4. **Audit Trail**: Complete logging of all security events
5. **Cryptographic Integrity**: All security features use strong cryptography

## 3. Capability Attestation

### 3.1 Attestation Structure

```rust
pub struct CapabilityAttestation {
    pub signature: String,
    pub authority: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}
```

### 3.2 Attestation Process

#### 3.2.1 Attestation Creation

```rust
fn create_capability_attestation(
    capability_manifest: &CapabilityManifest,
    private_key: &PrivateKey,
    authority: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<CapabilityAttestation, SecurityError> {
    // Create attestation payload
    let payload = create_attestation_payload(capability_manifest);
    
    // Sign the payload
    let signature = sign_payload(&payload, private_key)?;
    
    // Create attestation
    Ok(CapabilityAttestation {
        signature,
        authority: authority.to_string(),
        created_at: Utc::now(),
        expires_at,
        metadata: HashMap::new(),
    })
}
```

#### 3.2.2 Attestation Verification

```rust
async fn verify_capability_attestation(
    &self,
    attestation: &CapabilityAttestation,
    manifest: &CapabilityManifest,
) -> Result<bool, RuntimeError> {
    // Check expiration
    if let Some(expires_at) = attestation.expires_at {
        if Utc::now() > expires_at {
            return Ok(false);
        }
    }
    
    // Verify signature
    let payload = create_attestation_payload(manifest);
    let public_key = get_authority_public_key(&attestation.authority).await?;
    
    verify_signature(&payload, &attestation.signature, &public_key)
}
```

### 3.3 Attestation Authorities

#### 3.3.1 Authority Types

- **Trusted Authorities**: Pre-configured trusted signing authorities
- **Self-Signed**: Capabilities signed by their own providers
- **Federated Authorities**: Authorities from trusted federations
- **Community Authorities**: Community-verified authorities

#### 3.3.2 Authority Management

```rust
pub struct AttestationAuthority {
    pub id: String,
    pub name: String,
    pub public_key: PublicKey,
    pub trust_level: TrustLevel,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
}

pub enum TrustLevel {
    High,    // System authorities
    Medium,  // Verified authorities
    Low,     // Community authorities
    Unknown, // Unverified authorities
}
```

## 4. Provenance Tracking

### 4.1 Provenance Structure

```rust
pub struct CapabilityProvenance {
    pub source: String,
    pub version: Option<String>,
    pub content_hash: String,
    pub custody_chain: Vec<String>,
    pub registered_at: DateTime<Utc>,
}
```

### 4.2 Provenance Chain

The provenance chain tracks the complete history of a capability:

```
Original Source → Registry 1 → Registry 2 → Local Marketplace
     |              |            |            |
  content_hash   content_hash  content_hash  content_hash
     |              |            |            |
  attestation    attestation   attestation   attestation
```

### 4.3 Provenance Verification

```rust
async fn verify_capability_provenance(
    &self,
    provenance: &CapabilityProvenance,
    manifest: &CapabilityManifest,
) -> Result<bool, RuntimeError> {
    // Verify content hash
    let computed_hash = self.compute_content_hash(&manifest.to_string());
    if computed_hash != provenance.content_hash {
        return Ok(false);
    }
    
    // Verify custody chain
    for custody_entry in &provenance.custody_chain {
        if !self.verify_custody_entry(custody_entry).await? {
            return Ok(false);
        }
    }
    
    Ok(true)
}
```

## 5. Content Hashing

### 5.1 Hash Algorithm

The system uses **SHA-256** for content hashing:

```rust
fn compute_content_hash(&self, content: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}
```

### 5.2 Hash Verification

```rust
fn verify_content_hash(&self, content: &str, expected_hash: &str) -> bool {
    let computed_hash = self.compute_content_hash(content);
    computed_hash == expected_hash
}
```

### 5.3 Hash Usage

Content hashes are used for:

- **Integrity Verification**: Ensure capability hasn't been tampered with
- **Deduplication**: Identify identical capabilities
- **Caching**: Cache capabilities by content hash
- **Audit Trail**: Track capability modifications

## 6. Schema Validation

### 6.1 RTFS Native Type Validation

All capabilities support schema validation using RTFS native types:

```rust
// Input schema validation
async fn validate_input_schema(
    &self,
    params: &HashMap<String, Value>,
    schema_expr: &TypeExpr,
) -> Result<(), RuntimeError> {
    let value = self.params_to_value(params)?;
    self.type_validator.validate_value(&value, schema_expr)
}

// Output schema validation
async fn validate_output_schema(
    &self,
    result: &Value,
    schema_expr: &TypeExpr,
) -> Result<(), RuntimeError> {
    self.type_validator.validate_value(result, schema_expr)
}
```

### 6.2 Validation Examples

#### 6.2.1 Simple String Validation

**Rust Schema Definition:**
```rust
let input_schema = TypeExpr::Primitive(PrimitiveType::String);
```

**RTFS Schema Definition:**
```rtfs
string
```

**Validation Code:**
```rust
let input = Value::String("hello".to_string());
type_validator.validate_value(&input, &input_schema)?;
```

#### 6.2.2 Map Validation with Required Fields

**Rust Schema Definition:**
```rust
let input_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("name".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("age".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Number)),
            optional: false,
        }
    ],
    wildcard: None,
};
```

**RTFS Schema Definition:**
```rtfs
[:map [:name string] [:age float]]
```

**Validation Code:**
```rust
let mut input_map = HashMap::new();
input_map.insert(MapKey::Keyword("name".to_string()), Value::String("Alice".to_string()));
input_map.insert(MapKey::Keyword("age".to_string()), Value::Number(30.0));
let input = Value::Map(input_map);

type_validator.validate_value(&input, &input_schema)?;
```

#### 6.2.3 Map Validation with Optional Fields

**Rust Schema Definition:**
```rust
let input_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("name".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: false,
        },
        MapTypeEntry {
            key: Keyword("email".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            optional: true,
        }
    ],
    wildcard: None,
};
```

**RTFS Schema Definition:**
```rtfs
[:map [:name string] [:email string ?]]
```

#### 6.2.4 Complex Nested Validation

**Rust Schema Definition:**
```rust
let input_schema = TypeExpr::Map {
    entries: vec![
        MapTypeEntry {
            key: Keyword("user".to_string()),
            value_type: Box::new(TypeExpr::Map {
                entries: vec![
                    MapTypeEntry {
                        key: Keyword("id".to_string()),
                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Number)),
                        optional: false,
                    },
                    MapTypeEntry {
                        key: Keyword("permissions".to_string()),
                        value_type: Box::new(TypeExpr::List {
                            element_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                        }),
                        optional: true,
                    }
                ],
                wildcard: None,
            }),
            optional: false,
        }
    ],
    wildcard: None,
};
```

**RTFS Schema Definition:**
```rtfs
[:map [:user [:map [:id float] [:permissions [:vector string] ?]]]]
```

#### 6.2.5 Union Type Validation

**Rust Schema Definition:**
```rust
let input_schema = TypeExpr::Union {
    variants: vec![
        TypeExpr::Primitive(PrimitiveType::String),
        TypeExpr::Primitive(PrimitiveType::Number),
        TypeExpr::Map {
            entries: vec![
                MapTypeEntry {
                    key: Keyword("error".to_string()),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                    optional: false,
                }
            ],
            wildcard: None,
        }
    ],
};
```

**RTFS Schema Definition:**
```rtfs
[:union string float [:map [:error string]]]
```

### 6.3 Validation Configuration

```rust
pub struct TypeCheckingConfig {
    pub strict_mode: bool,
    pub allow_unknown_fields: bool,
    pub verify_attestations: bool,
    pub verify_provenance: bool,
    pub max_validation_depth: usize,
}
```

## 7. Permission System

### 7.1 Permission Structure

```rust
pub struct CapabilityPermissions {
    pub read: bool,
    pub execute: bool,
    pub modify: bool,
    pub delete: bool,
    pub share: bool,
    pub custom_permissions: HashMap<String, bool>,
}
```

### 7.2 Permission Checking

```rust
fn check_capability_permission(
    &self,
    capability: &CapabilityManifest,
    permission: &str,
    security_context: &SecurityContext,
) -> Result<bool, RuntimeError> {
    // Check explicit permissions
    if capability.permissions.contains(&permission.to_string()) {
        return Ok(true);
    }
    
    // Check security context
    if security_context.has_permission(permission) {
        return Ok(true);
    }
    
    // Check role-based permissions
    for role in &security_context.roles {
        if role.has_permission(permission) {
            return Ok(true);
        }
    }
    
    Ok(false)
}
```

### 7.3 Security Context

```rust
pub struct SecurityContext {
    pub user_id: Option<String>,
    pub roles: Vec<Role>,
    pub permissions: HashSet<String>,
    pub session_id: Option<String>,
    pub ip_address: Option<String>,
    pub timestamp: DateTime<Utc>,
}

pub struct Role {
    pub name: String,
    pub permissions: HashSet<String>,
    pub scope: PermissionScope,
}

pub enum PermissionScope {
    Global,
    Organization(String),
    Project(String),
    Capability(String),
}
```

## 8. Security Configuration

### 8.1 Security Settings

```rust
pub struct SecurityConfig {
    pub attestation_required: bool,
    pub provenance_required: bool,
    pub schema_validation_required: bool,
    pub trusted_authorities: Vec<String>,
    pub max_attestation_age: Duration,
    pub content_hash_verification: bool,
    pub permission_enforcement: bool,
    pub audit_logging: bool,
}
```

### 8.2 Security Levels

```rust
pub enum SecurityLevel {
    Low,    // Minimal security checks
    Medium, // Standard security checks
    High,   // Strict security checks
    Maximum, // Maximum security enforcement
}
```

## 9. Audit Logging

### 9.1 Audit Events

```rust
pub enum SecurityAuditEvent {
    CapabilityRegistered {
        capability_id: String,
        source: String,
        timestamp: DateTime<Utc>,
    },
    CapabilityExecuted {
        capability_id: String,
        user_id: Option<String>,
        timestamp: DateTime<Utc>,
        success: bool,
    },
    AttestationVerified {
        capability_id: String,
        authority: String,
        timestamp: DateTime<Utc>,
        success: bool,
    },
    PermissionDenied {
        capability_id: String,
        user_id: Option<String>,
        permission: String,
        timestamp: DateTime<Utc>,
    },
}
```

### 9.2 Audit Logging

```rust
pub trait SecurityAuditLogger: Send + Sync {
    async fn log_event(&self, event: SecurityAuditEvent) -> Result<(), RuntimeError>;
    async fn query_events(&self, filter: AuditEventFilter) -> Result<Vec<SecurityAuditEvent>, RuntimeError>;
}

pub struct AuditEventFilter {
    pub capability_id: Option<String>,
    pub user_id: Option<String>,
    pub event_type: Option<AuditEventType>,
    pub from_timestamp: Option<DateTime<Utc>>,
    pub to_timestamp: Option<DateTime<Utc>>,
}
```

## 10. Error Handling

### 10.1 Security Error Types

```rust
pub enum SecurityError {
    AttestationExpired { capability_id: String, expires_at: DateTime<Utc> },
    AttestationInvalid { capability_id: String, reason: String },
    AuthorityUnknown { authority: String },
    ContentHashMismatch { expected: String, actual: String },
    PermissionDenied { capability_id: String, permission: String },
    SchemaValidationFailed { field: String, reason: String },
    ProvenanceInvalid { capability_id: String, reason: String },
}
```

### 10.2 Error Recovery

```rust
impl CapabilityMarketplace {
    async fn execute_with_security_fallback(
        &self,
        capability_id: &str,
        inputs: &Value,
        security_config: &SecurityConfig,
    ) -> Result<Value, RuntimeError> {
        match self.execute_with_full_security(capability_id, inputs, security_config).await {
            Ok(result) => Ok(result),
            Err(SecurityError::AttestationExpired { .. }) => {
                // Try without attestation verification
                self.execute_without_attestation(capability_id, inputs).await
            }
            Err(SecurityError::PermissionDenied { .. }) => {
                // Try with elevated permissions
                self.execute_with_elevated_permissions(capability_id, inputs).await
            }
            Err(e) => Err(RuntimeError::SecurityError(e)),
        }
    }
}
```

## 11. Testing

### 11.1 Security Testing

```rust
#[tokio::test]
async fn test_attestation_verification() {
    let marketplace = create_test_marketplace();
    
    // Create capability with attestation
    let capability = create_test_capability_with_attestation();
    marketplace.register_capability(capability).await.unwrap();
    
    // Verify attestation
    let result = marketplace.execute_with_validation("test_capability", &params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_permission_enforcement() {
    let marketplace = create_test_marketplace();
    let security_context = SecurityContext {
        user_id: Some("user1".to_string()),
        permissions: HashSet::new(),
        roles: vec![],
        session_id: None,
        ip_address: None,
        timestamp: Utc::now(),
    };
    
    // Try to execute capability without permission
    let result = marketplace.execute_with_security_context(
        "restricted_capability",
        &params,
        &security_context
    ).await;
    
    assert!(result.is_err());
}
```

### 11.2 Penetration Testing

```rust
#[tokio::test]
async fn test_tamper_detection() {
    let marketplace = create_test_marketplace();
    
    // Register capability
    let capability = create_test_capability();
    marketplace.register_capability(capability).await.unwrap();
    
    // Tamper with capability
    marketplace.tamper_with_capability("test_capability").await;
    
    // Verify tampering is detected
    let result = marketplace.execute_capability("test_capability", &params).await;
    assert!(result.is_err());
}
```

## 12. Deployment Security

### 12.1 Production Security Checklist

- [ ] **TLS Configuration**: Enable TLS 1.3 for all network communication
- [ ] **Certificate Management**: Use valid certificates from trusted CAs
- [ ] **Key Management**: Secure storage of private keys and secrets
- [ ] **Access Control**: Implement proper authentication and authorization
- [ ] **Audit Logging**: Enable comprehensive audit logging
- [ ] **Monitoring**: Set up security monitoring and alerting
- [ ] **Backup**: Regular backup of security configurations
- [ ] **Updates**: Keep security components updated

### 12.2 Security Hardening

```rust
pub struct SecurityHardening {
    pub enable_tls: bool,
    pub certificate_pinning: bool,
    pub secure_headers: bool,
    pub rate_limiting: bool,
    pub input_sanitization: bool,
    pub output_encoding: bool,
}
```

## 13. Compliance

### 13.1 Security Standards

The RTFS 2.0 Security System complies with:

- **OWASP Top 10**: Addresses common web application security risks
- **NIST Cybersecurity Framework**: Follows security best practices
- **ISO 27001**: Information security management standards
- **SOC 2**: Security, availability, and confidentiality controls

### 13.2 Compliance Reporting

```rust
pub struct ComplianceReport {
    pub attestation_coverage: f64,
    pub provenance_tracking: bool,
    pub audit_logging_enabled: bool,
    pub security_controls: Vec<SecurityControl>,
    pub compliance_score: f64,
}

pub struct SecurityControl {
    pub name: String,
    pub status: ControlStatus,
    pub last_verified: DateTime<Utc>,
    pub description: String,
}

pub enum ControlStatus {
    Implemented,
    PartiallyImplemented,
    NotImplemented,
    NotApplicable,
}
```

## 14. Future Security Enhancements

### 14.1 Planned Features

- **Hardware Security Modules (HSM)**: Integration with HSM for key management
- **Zero-Knowledge Proofs**: Privacy-preserving capability verification
- **Blockchain Integration**: Immutable capability provenance tracking
- **AI-Powered Threat Detection**: Machine learning-based security monitoring

### 14.2 Security Research

- **Post-Quantum Cryptography**: Preparation for quantum-resistant algorithms
- **Advanced Threat Modeling**: Comprehensive security analysis
- **Security Metrics**: Quantitative security measurement
- **Incident Response**: Automated security incident handling

---

**Note**: This specification defines the complete RTFS 2.0 security and attestation system based on the implementation completed in Issue #43. 