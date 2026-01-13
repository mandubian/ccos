# Capability Importers and Synthesis

**Status**: Under Review (updated for architecture changes)  
**Version**: 1.1  
**Last Updated**: 2025-12-25  
**Scope**: API importers, capability synthesis, and RTFS code generation

---

## 1. Overview

The Capability Importers and Synthesis system converts external API specifications into native CCOS capabilities. This enables CCOS to consume:

- **OpenAPI/Swagger specifications** → HTTP-based capabilities
- **GraphQL schemas** → Query/mutation capabilities
- **MCP servers** → Tool-based capabilities (see 031-mcp-discovery-unified-service.md)
- **A2A agent cards** → Agent-to-agent capabilities
- **Custom APIs** → HTTP wrapper capabilities

Additionally, the synthesis system can generate pure RTFS implementations when no external API is available.

---

## 2. Importer Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Capability Importers                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │   OpenAPI    │  │   GraphQL    │  │     MCP      │           │
│  │   Importer   │  │   Importer   │  │  Introspector│           │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘           │
│         │                 │                  │                   │
│         └─────────────────┼──────────────────┘                   │
│                           ▼                                      │
│              ┌────────────────────────┐                          │
│              │   Schema Converter     │                          │
│              │  (JSON → RTFS TypeExpr)│                          │
│              └───────────┬────────────┘                          │
│                          ▼                                       │
│              ┌────────────────────────┐                          │
│              │  Manifest Generator    │                          │
│              │  (CapabilityManifest)  │                          │
│              └───────────┬────────────┘                          │
│                          ▼                                       │
│              ┌────────────────────────┐                          │
│              │   RTFS Code Generator  │                          │
│              │  (capability def + impl)│                          │
│              └────────────────────────┘                          │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. OpenAPI Importer

### 3.1 Overview

Converts OpenAPI 3.x specifications to CCOS capabilities. Each operation becomes a separate capability.

### 3.2 Core Types

```rust
pub struct OpenAPIImporter {
    http_client: reqwest::Client,
    base_url: Option<String>,
    auth_injector: AuthInjector,
}

pub struct OpenAPIOperation {
    pub path: String,              // "/repos/{owner}/{repo}/issues"
    pub method: String,            // "GET", "POST", etc.
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub parameters: Vec<OpenAPIParameter>,
    pub request_body: Option<OpenAPIRequestBody>,
    pub responses: HashMap<String, OpenAPIResponse>,
    pub security: Vec<OpenAPISecurityRequirement>,
}

pub struct OpenAPIParameter {
    pub name: String,
    pub location: ParameterLocation,  // Path, Query, Header, Cookie
    pub required: bool,
    pub schema: serde_json::Value,
    pub description: Option<String>,
}
```

### 3.3 Import Flow

```rust
impl OpenAPIImporter {
    /// Import all operations from an OpenAPI spec
    pub async fn import_spec(&self, spec_url: &str) -> RuntimeResult<Vec<CapabilityManifest>> {
        // 1. Fetch and parse spec
        let spec: OpenAPISpec = self.fetch_spec(spec_url).await?;
        
        // 2. Extract base URL
        let base_url = self.extract_base_url(&spec);
        
        // 3. Convert each operation
        let mut capabilities = Vec::new();
        for (path, path_item) in &spec.paths {
            for (method, operation) in path_item.operations() {
                let manifest = self.operation_to_capability(
                    &operation,
                    path,
                    method,
                    &base_url,
                )?;
                capabilities.push(manifest);
            }
        }
        
        Ok(capabilities)
    }
    
    /// Convert a single operation to capability
    pub fn operation_to_capability(
        &self,
        operation: &OpenAPIOperation,
        path: &str,
        method: &str,
        base_url: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        // Generate capability ID
        let id = self.generate_id(operation, path, method);
        
        // Convert parameters to input schema
        let input_schema = self.parameters_to_type_expr(&operation.parameters)?;
        
        // Convert response to output schema
        let output_schema = self.response_to_type_expr(&operation.responses)?;
        
        // Build provider
        let provider = ProviderType::OpenApi(OpenApiCapability {
            base_url: base_url.to_string(),
            spec_url: Some(self.spec_url.clone()),
            operations: vec![operation.to_openapi_operation()],
            auth: self.build_auth_config(&operation.security),
            timeout_ms: 30000,
        });
        
        Ok(CapabilityManifest {
            id,
            name: operation.operation_id.clone().unwrap_or_else(|| 
                format!("{}_{}", method.to_lowercase(), path.replace("/", "_"))
            ),
            description: operation.summary.clone().unwrap_or_default(),
            provider,
            version: "1.0.0".to_string(),
            input_schema: Some(input_schema),
            output_schema,
            domains: self.infer_domains(path, operation),
            categories: self.infer_categories(method),
            ..Default::default()
        })
    }
}
```

### 3.4 Schema Conversion

```rust
impl OpenAPIImporter {
    /// Convert OpenAPI schema to RTFS TypeExpr
    fn json_schema_to_type_expr(&self, schema: &serde_json::Value) -> RuntimeResult<TypeExpr> {
        match schema.get("type").and_then(|t| t.as_str()) {
            Some("string") => Ok(TypeExpr::Primitive(PrimitiveType::String)),
            Some("integer") => Ok(TypeExpr::Primitive(PrimitiveType::Int)),
            Some("number") => Ok(TypeExpr::Primitive(PrimitiveType::Float)),
            Some("boolean") => Ok(TypeExpr::Primitive(PrimitiveType::Bool)),
            Some("array") => {
                let items = schema.get("items").unwrap_or(&serde_json::json!({}));
                let item_type = self.json_schema_to_type_expr(items)?;
                Ok(TypeExpr::Vector(Box::new(item_type)))
            }
            Some("object") => {
                let properties = schema.get("properties")
                    .and_then(|p| p.as_object())
                    .unwrap_or(&serde_json::Map::new());
                let required = schema.get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<HashSet<_>>())
                    .unwrap_or_default();
                
                let entries = properties.iter().map(|(key, value)| {
                    let key_keyword = MapKey::Keyword(Keyword::new(key.clone()));
                    let value_type = self.json_schema_to_type_expr(value)?;
                    let is_required = required.contains(key.as_str());
                    Ok(MapTypeEntry {
                        key: key_keyword,
                        value_type,
                        optional: !is_required,
                    })
                }).collect::<RuntimeResult<Vec<_>>>()?;
                
                Ok(TypeExpr::Map(entries))
            }
            _ => Ok(TypeExpr::Any),
        }
    }
}
```

### 3.5 Generated RTFS

```clojure
;; Generated from OpenAPI: GET /repos/{owner}/{repo}/issues
(capability :openapi.github.list_issues
  :version "1.0.0"
  :description "List issues in a repository"
  
  :input-schema [:map
    [:owner :string]
    [:repo :string]
    [:state {:optional true} :string]
    [:labels {:optional true} :string]
    [:per_page {:optional true} :int]]
  
  :output-schema [:vector [:map
    [:id :int]
    [:number :int]
    [:title :string]
    [:state :string]
    [:body {:optional true} :string]]]
  
  :provider {:type :openapi
             :base-url "https://api.github.com"
             :spec-url "https://api.github.com/openapi.json"
             :operation {:method "GET"
                         :path "/repos/{owner}/{repo}/issues"}}
  
  :domains ["github" "github.issues"]
  :categories ["crud.read"])
```

---

## 4. GraphQL Importer

### 4.1 Overview

Introspects GraphQL endpoints and generates capabilities for queries and mutations.

### 4.2 Core Types

```rust
pub struct GraphQLImporter {
    http_client: reqwest::Client,
}

pub struct GraphQLField {
    pub name: String,
    pub description: Option<String>,
    pub args: Vec<GraphQLArgument>,
    pub return_type: GraphQLType,
}

pub struct GraphQLArgument {
    pub name: String,
    pub arg_type: GraphQLType,
    pub default_value: Option<serde_json::Value>,
}

pub enum GraphQLType {
    Scalar(String),
    Object(String),
    List(Box<GraphQLType>),
    NonNull(Box<GraphQLType>),
    Enum(String, Vec<String>),
}
```

### 4.3 Introspection

```rust
impl GraphQLImporter {
    /// Introspect a GraphQL endpoint
    pub async fn introspect(&self, endpoint: &str) -> RuntimeResult<GraphQLSchema> {
        let introspection_query = r#"
            query IntrospectionQuery {
                __schema {
                    queryType { name fields { ...FieldInfo } }
                    mutationType { name fields { ...FieldInfo } }
                }
            }
            fragment FieldInfo on __Field {
                name
                description
                args { name type { ...TypeRef } defaultValue }
                type { ...TypeRef }
            }
            fragment TypeRef on __Type {
                kind name
                ofType { kind name ofType { kind name } }
            }
        "#;
        
        let response = self.http_client
            .post(endpoint)
            .json(&json!({ "query": introspection_query }))
            .send()
            .await?;
        
        self.parse_introspection_response(response.json().await?)
    }
    
    /// Convert a GraphQL field to a capability
    pub fn field_to_capability(
        &self,
        field: &GraphQLField,
        operation_type: &str,  // "query" or "mutation"
        endpoint: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        let id = format!("graphql.{}.{}", operation_type, field.name);
        
        // Convert args to input schema
        let input_schema = self.args_to_type_expr(&field.args)?;
        
        // Convert return type to output schema
        let output_schema = self.graphql_type_to_type_expr(&field.return_type)?;
        
        // Determine category
        let category = if operation_type == "mutation" {
            "crud.write"
        } else {
            "crud.read"
        };
        
        Ok(CapabilityManifest {
            id: id.clone(),
            name: field.name.clone(),
            description: field.description.clone().unwrap_or_default(),
            provider: ProviderType::Http(HttpCapability {
                base_url: endpoint.to_string(),
                auth_token: None,
                timeout_ms: 30000,
            }),
            version: "1.0.0".to_string(),
            input_schema: Some(input_schema),
            output_schema: Some(output_schema),
            categories: vec![category.to_string()],
            metadata: {
                let mut m = HashMap::new();
                m.insert("graphql_operation".to_string(), operation_type.to_string());
                m.insert("graphql_field".to_string(), field.name.clone());
                m
            },
            ..Default::default()
        })
    }
}
```

### 4.4 Type Conversion

```rust
impl GraphQLImporter {
    fn graphql_type_to_type_expr(&self, gql_type: &GraphQLType) -> RuntimeResult<TypeExpr> {
        match gql_type {
            GraphQLType::Scalar(name) => match name.as_str() {
                "String" | "ID" => Ok(TypeExpr::Primitive(PrimitiveType::String)),
                "Int" => Ok(TypeExpr::Primitive(PrimitiveType::Int)),
                "Float" => Ok(TypeExpr::Primitive(PrimitiveType::Float)),
                "Boolean" => Ok(TypeExpr::Primitive(PrimitiveType::Bool)),
                _ => Ok(TypeExpr::Any),
            },
            GraphQLType::Object(name) => {
                // Look up object definition in schema
                // For now, return Any as placeholder
                Ok(TypeExpr::Any)
            },
            GraphQLType::List(inner) => {
                let inner_type = self.graphql_type_to_type_expr(inner)?;
                Ok(TypeExpr::Vector(Box::new(inner_type)))
            },
            GraphQLType::NonNull(inner) => {
                self.graphql_type_to_type_expr(inner)
            },
            GraphQLType::Enum(name, values) => {
                // Enums become strings with validation
                Ok(TypeExpr::Primitive(PrimitiveType::String))
            },
        }
    }
}
```

---

## 5. HTTP Wrapper

### 5.1 Overview

Wraps arbitrary HTTP APIs that don't have OpenAPI/GraphQL specs.

### 5.2 Template System

```rust
pub struct HTTPWrapper {
    templates: HashMap<String, HTTPTemplate>,
}

pub struct HTTPTemplate {
    pub method: String,
    pub url_pattern: String,           // "https://api.example.com/users/{id}"
    pub headers: HashMap<String, String>,
    pub query_params: Vec<String>,
    pub body_template: Option<String>, // JSON template with placeholders
    pub response_path: Option<String>, // JSONPath to extract data
}

impl HTTPWrapper {
    /// Create a capability from an HTTP template
    pub fn create_capability(
        &self,
        name: &str,
        template: HTTPTemplate,
        input_schema: TypeExpr,
        output_schema: TypeExpr,
    ) -> CapabilityManifest {
        CapabilityManifest {
            id: format!("http.{}", name),
            name: name.to_string(),
            description: format!("HTTP {} {}", template.method, template.url_pattern),
            provider: ProviderType::Http(HttpCapability {
                base_url: template.url_pattern.clone(),
                auth_token: None,
                timeout_ms: 30000,
            }),
            version: "1.0.0".to_string(),
            input_schema: Some(input_schema),
            output_schema: Some(output_schema),
            metadata: {
                let mut m = HashMap::new();
                m.insert("http_method".to_string(), template.method.clone());
                m.insert("http_url_pattern".to_string(), template.url_pattern.clone());
                m
            },
            ..Default::default()
        }
    }
}
```

### 5.3 RTFS Generation for HTTP

```clojure
;; Generated HTTP wrapper capability
(capability :http.get_user
  :version "1.0.0"
  :description "HTTP GET https://api.example.com/users/{id}"
  
  :input-schema [:map
    [:id :string]
    [:include_profile {:optional true} :bool]]
  
  :output-schema [:map
    [:id :string]
    [:name :string]
    [:email :string]]
  
  :implementation
  (fn [input]
    (let [url (str "https://api.example.com/users/" (:id input))
          query (if (:include_profile input)
                  "?include=profile"
                  "")
          response (call :ccos.network.http-fetch
                    {:url (str url query)
                     :method "GET"
                     :headers {"Authorization" (get-env "API_TOKEN")}})]
      (get response :body))))
```

---

## 6. Auth Injection

### 6.1 Auth Types

```rust
pub enum AuthType {
    Bearer,                    // Authorization: Bearer <token>
    Basic,                     // Authorization: Basic <base64>
    ApiKey { header: String }, // Custom header with key
    OAuth2 { flow: String },   // OAuth2 flow
    Custom { template: String }, // Custom auth template
}

pub struct AuthConfig {
    pub auth_type: AuthType,
    pub token_source: TokenSource,
    pub refresh_enabled: bool,
}

pub enum TokenSource {
    EnvVar(String),           // Read from environment variable
    File(PathBuf),            // Read from file
    Keychain(String),         // Read from system keychain
    Runtime,                  // Provided at runtime
}
```

### 6.2 Auth Injector

```rust
pub struct AuthInjector;

impl AuthInjector {
    /// Generate RTFS code for auth injection
    pub fn generate_auth_code(&self, config: &AuthConfig) -> String {
        match &config.auth_type {
            AuthType::Bearer => {
                match &config.token_source {
                    TokenSource::EnvVar(var) => {
                        format!(r#"(let [token (get-env "{}")] 
                                    {{"Authorization" (str "Bearer " token)}})"#, var)
                    }
                    _ => unimplemented!()
                }
            }
            AuthType::ApiKey { header } => {
                match &config.token_source {
                    TokenSource::EnvVar(var) => {
                        format!(r#"(let [key (get-env "{}")] 
                                    {{"{}" key}})"#, var, header)
                    }
                    _ => unimplemented!()
                }
            }
            _ => unimplemented!()
        }
    }
}
```

---

## 7. Capability Synthesis (Updated Architecture)

> [!IMPORTANT]
> **Architecture Change (December 2025)**: Discovery and synthesis are now separate concerns.
> - **Discovery** = finding existing capabilities (marketplace, MCP)
> - **Synthesis** = creating new capabilities (via `planner.synthesize_capability`)

### 7.1 Separation of Concerns

```
┌─────────────────────────────────────────────────────────────────┐
│                     Discovery (Pure Lookup)                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  CapabilityNeed ──► DiscoveryEngine ──► Result                    │
│                                                                   │
│  [1] Search local marketplace                                     │
│  [2] Search MCP registry                                          │
│  [3] Return NotFound (no synthesis in discovery)                  │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
                            │
                            ▼ (if NotFound)
┌─────────────────────────────────────────────────────────────────┐
│                     Synthesis (Governance-Gated)                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  CapabilityNeed ──► GovernanceKernel ──► Planner Capability       │
│                                                                   │
│  [1] Risk assessment (SynthesisRiskAssessment)                    │
│  [2] Authorization check (check_synthesis_authorization)          │
│  [3] If allowed: invoke planner.synthesize_capability             │
│  [4] Validate and register synthesized capability                 │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

### 7.2 Governance-Gated Synthesis

Synthesis is now gated by the `GovernanceKernel` to ensure security and compliance:

```rust
// In governance_kernel.rs
pub fn check_synthesis_authorization(&self, capability_id: &str) -> RuleAction {
    let assessment = SynthesisRiskAssessment::assess(capability_id);

    match assessment.risk {
        SynthesisRisk::Low => RuleAction::Allow,
        SynthesisRisk::Medium => {
            log::info!("Medium-risk synthesis allowed: {}", capability_id);
            RuleAction::Allow
        }
        SynthesisRisk::High => {
            if assessment.requires_human_approval {
                RuleAction::RequireHumanApproval
            } else {
                RuleAction::Allow
            }
        }
        SynthesisRisk::Critical => {
            RuleAction::Deny(format!("Critical risk synthesis blocked: {}", capability_id))
        }
    }
}
```

### 7.3 Risk Assessment

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum SynthesisRisk {
    Low,      // Safe primitives only (math, string manipulation)
    Medium,   // Network calls, file I/O with restrictions
    High,     // External API calls, credential usage
    Critical, // System modification, privilege escalation
}

pub struct SynthesisRiskAssessment {
    pub risk: SynthesisRisk,
    pub risk_factors: Vec<String>,
    pub security_concerns: Vec<String>,
    pub compliance_requirements: Vec<String>,
    pub requires_human_approval: bool,
}
```

### 7.4 Planner Synthesis Capability

Synthesis is invoked via `planner.synthesize_capability`:

```clojure
;; Define the synthesis capability
(capability :planner.synthesize_capability
  :version "1.0.0"
  :description "Synthesize a new capability from requirements"
  
  :input-schema [:map
    [:capability_id :string]
    [:description :string]
    [:input_requirements [:vector :string]]
    [:output_requirements [:vector :string]]
    [:constraints {:optional true} [:vector :string]]]
  
  :output-schema [:map
    [:manifest :any]
    [:rtfs_code :string]
    [:risk_assessment :any]]
  
  :effects [:write :synthesize])
```

### 7.5 Synthesis Prompts (LLM-Based)

```rust
impl PlannerCapabilities {
    fn build_synthesis_prompt(
        &self,
        need: &CapabilityNeed,
        context: &SynthesisContext,
    ) -> String {
        format!(r#"
Generate an RTFS capability definition for the following need:

**Capability Need:**
- ID: {}
- Description: {}
- Required inputs: {:?}
- Expected output: {:?}

**Available Primitives:**
{}

**Constraints:**
- Use only the primitives listed above
- Do not use external APIs unless explicitly allowed
- Keep the implementation pure and deterministic where possible
- Follow RTFS syntax exactly

Generate a complete (capability ...) form:
"#, 
            need.id,
            need.description,
            need.required_inputs,
            need.expected_output,
            self.format_available_primitives(context)
        )
    }
}
```

### 7.6 Available Primitives for Synthesis

```rust
const SYNTHESIS_PRIMITIVES: &[&str] = &[
    // I/O
    "println, log, tool/log, tool/time-ms",
    
    // Arithmetic
    "+, -, *, /, mod, zero?, =, <, >, <=, >=",
    
    // Collections
    "map, filter, reduce, sort-by, group-by",
    "first, rest, nth, count, empty?, conj",
    "assoc, dissoc, get, keys, vals, merge",
    
    // Strings
    "str, string-lower, string-upper, string-contains",
    "string-split, string-join, string-trim",
    
    // Control flow
    "if, cond, when, let, do, fn",
    
    // Type checks
    "string?, number?, map?, vector?, nil?",
];
```

### 7.7 Deprecated Components

> [!WARNING]
> The following components have been removed from the synthesis flow:
> - **`LocalSynthesizer`**: Rule-based synthesis replaced by LLM-based approach
> - **`RecursiveSynthesizer` in discovery**: Synthesis removed from discovery chain
> - **Pattern-based synthesis fallbacks**: Now handled by planner capabilities

---

## 8. A2A Importer

### 8.1 Overview

Imports capabilities from A2A (Agent-to-Agent) protocol agent cards.

### 8.2 Agent Card Structure

```rust
pub struct A2AAgentCard {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<A2ACapabilityDef>,
    pub endpoints: A2AEndpoints,
    pub auth_requirements: Option<A2AAuth>,
}

pub struct A2ACapabilityDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
}
```

### 8.3 Import Flow

```rust
pub struct A2AImporter {
    http_client: reqwest::Client,
}

impl A2AImporter {
    /// Import capabilities from an A2A agent
    pub async fn import_agent(
        &self,
        agent_url: &str,
    ) -> RuntimeResult<Vec<CapabilityManifest>> {
        // Fetch agent card
        let card: A2AAgentCard = self.http_client
            .get(format!("{}/.well-known/agent.json", agent_url))
            .send()
            .await?
            .json()
            .await?;
        
        // Convert each capability
        let capabilities = card.capabilities.iter()
            .map(|cap| self.capability_to_manifest(cap, &card))
            .collect::<RuntimeResult<Vec<_>>>()?;
        
        Ok(capabilities)
    }
    
    fn capability_to_manifest(
        &self,
        cap: &A2ACapabilityDef,
        card: &A2AAgentCard,
    ) -> RuntimeResult<CapabilityManifest> {
        Ok(CapabilityManifest {
            id: format!("a2a.{}.{}", card.agent_id, cap.id),
            name: cap.name.clone(),
            description: cap.description.clone(),
            provider: ProviderType::A2A(A2ACapability {
                agent_url: card.endpoints.invoke.clone(),
                agent_id: card.agent_id.clone(),
                timeout_ms: 60000,
            }),
            version: "1.0.0".to_string(),
            input_schema: Some(self.json_to_type_expr(&cap.input_schema)?),
            output_schema: Some(self.json_to_type_expr(&cap.output_schema)?),
            ..Default::default()
        })
    }
}
```

---

## 9. RTFS Code Generation

### 9.1 Capability Definition Format

```clojure
(capability :namespace.capability-name
  ;; Metadata
  :version "1.0.0"
  :description "What this capability does"
  
  ;; Schema
  :input-schema [:map ...]
  :output-schema [:map ...]
  
  ;; Provider (for external capabilities)
  :provider {:type :mcp
             :server-url "..."
             :tool-name "..."}
  
  ;; OR Implementation (for pure RTFS)
  :implementation
  (fn [input]
    ...)
  
  ;; Classification
  :domains ["domain" "domain.subdomain"]
  :categories ["crud.read"]
  
  ;; Security
  :permissions ["network"]
  :effects ["read"])
```

### 9.2 Code Generator

```rust
pub struct RTFSCodeGenerator;

impl RTFSCodeGenerator {
    /// Generate RTFS code from a manifest
    pub fn generate_capability_code(&self, manifest: &CapabilityManifest) -> String {
        let mut code = String::new();
        
        // Open capability form
        code.push_str(&format!("(capability :{}\n", manifest.id));
        
        // Version
        code.push_str(&format!("  :version \"{}\"\n", manifest.version));
        
        // Description
        code.push_str(&format!("  :description \"{}\"\n", 
            manifest.description.replace("\"", "\\\"")));
        
        // Input schema
        if let Some(schema) = &manifest.input_schema {
            code.push_str(&format!("  :input-schema {}\n", 
                type_expr_to_rtfs_compact(schema)));
        }
        
        // Output schema
        if let Some(schema) = &manifest.output_schema {
            code.push_str(&format!("  :output-schema {}\n", 
                type_expr_to_rtfs_compact(schema)));
        }
        
        // Provider
        code.push_str(&format!("  :provider {}\n", 
            self.provider_to_rtfs(&manifest.provider)));
        
        // Domains
        if !manifest.domains.is_empty() {
            code.push_str(&format!("  :domains [{}]\n",
                manifest.domains.iter()
                    .map(|d| format!("\"{}\"", d))
                    .collect::<Vec<_>>()
                    .join(" ")));
        }
        
        // Categories
        if !manifest.categories.is_empty() {
            code.push_str(&format!("  :categories [{}]\n",
                manifest.categories.iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(" ")));
        }
        
        // Close capability form
        code.push_str(")\n");
        
        code
    }
    
    fn provider_to_rtfs(&self, provider: &ProviderType) -> String {
        match provider {
            ProviderType::MCP(mcp) => {
                format!(r#"{{:type :mcp :server-url "{}" :tool-name "{}"}}"#,
                    mcp.server_url, mcp.tool_name)
            }
            ProviderType::OpenApi(api) => {
                format!(r#"{{:type :openapi :base-url "{}"}}"#, api.base_url)
            }
            ProviderType::Http(http) => {
                format!(r#"{{:type :http :base-url "{}"}}"#, http.base_url)
            }
            _ => "{:type :local}".to_string()
        }
    }
}
```

---

## 9. Trace-to-Agent Synthesis (Consolidation)

> [!NOTE]
> This section addresses the conversion of linear execution traces (Sessions) into autonomous Agent Capabilities.

### 9.1 Overview

When a user or external agent completes a successful session in Interactive Mode, the linear log of steps (the **Trace**) can be synthesized into a reusable **Agent Capability**.
Unlike a simple composite capability (which replays steps exactly), an **Agent Capability** includes:
- **Autonomy**: Ability to adapt to new inputs using the original trace as a "Plan Template".
- **Governance**: Explicit `AgentMetadata` for risk control.
- **Effects**: Declared side-effects extracted from the trace.

### 9.2 The `planner.synthesize_agent_from_trace` Capability

This privileged capability is used to perform the consolidation.

```clojure
(capability :planner.synthesize_agent_from_trace
  :version "1.0.0"
  :description "Consolidate a session trace into a governed Agent Capability"
  
  :input-schema [:map
    [:session_trace_id :string]
    [:agent_name :string]
    [:description {:optional true} :string]
    [:generalize {:optional true} :bool]] ; If true, replace literals with parameters
    
  :output-schema [:map
    [:agent_id :string]
    [:manifest :any]
    [:validation_report :any]]
    
  :effects [:write :synthesize :configure])
```

### 9.3 Synthesis Process

1.  **Trace Analysis**:
    - The linear session is analyzed to identify input dependencies (where step N uses output of step M).
    - Side-effects (e.g., file writes, API calls) are aggregated.
    
2.  **Manifest Generation**:
    - **`:kind :agent`**: The artifact is marked as an Agent.
    - **`:planning true`**: Enabling the agent to use the Cognitive Engine if generalization is requested.
    - **`:effects [...]`**: Populated from the trace.
    
3.  **Governance Coupling**:
    - The new Agent ID is registered in the Governance Kernel.
    - Usage policies (e.g., "Requires Human Approval") are attached if the original trace involved high-risk actions.

### 9.4 Example: "Staging Deploy" Agent

**Source Trace**:
1. `(call :git.checkout {:branch "staging"})`
2. `(call :cargo.build {:profile "release"})`
3. `(call :aws.s3.upload {:bucket "builds" :file "target/release/app"})`

**Synthesized Agent Artifact**:
```rtfs
(capability :agent.deploy.staging.v1
  :description "Deploy to staging (Synthesized from Session #12345)"
  :parameters {:branch :string :bucket :string} ; Generalized from literals
  :metadata {
    :kind :agent
    :planning false    ; Strict replay for reliability
    :stateful false
    :source_session "session_12345"
  }
  :effects [:git_write :fs_read :network_write]
  :implementation
    (do
      (call :git.checkout {:branch branch})
      (call :cargo.build {:profile "release"})
      (call :aws.s3.upload {:bucket bucket :file "target/release/app"})))
```

---

## 10. Schema Serialization


### 10.1 Type Expression to RTFS String

```rust
pub fn type_expr_to_rtfs_compact(type_expr: &TypeExpr) -> String {
    match type_expr {
        TypeExpr::Primitive(p) => match p {
            PrimitiveType::String => ":string".to_string(),
            PrimitiveType::Int => ":int".to_string(),
            PrimitiveType::Float => ":float".to_string(),
            PrimitiveType::Bool => ":bool".to_string(),
            PrimitiveType::Nil => ":nil".to_string(),
        },
        TypeExpr::Any => ":any".to_string(),
        TypeExpr::Vector(inner) => {
            format!("[:vector {}]", type_expr_to_rtfs_compact(inner))
        }
        TypeExpr::Map(entries) => {
            let entries_str = entries.iter()
                .map(|e| {
                    let key = match &e.key {
                        MapKey::Keyword(k) => format!(":{}", k.name),
                        MapKey::String(s) => format!("\"{}\"", s),
                    };
                    if e.optional {
                        format!("[{} {{:optional true}} {}]", 
                            key, type_expr_to_rtfs_compact(&e.value_type))
                    } else {
                        format!("[{} {}]", key, type_expr_to_rtfs_compact(&e.value_type))
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("[:map {}]", entries_str)
        }
        _ => ":any".to_string(),
    }
}
```

---

## 11. File Locations

| Component | Location |
|-----------|----------|
| OpenAPI Importer | `ccos/src/synthesis/importers/openapi_importer.rs` |
| GraphQL Importer | `ccos/src/synthesis/importers/graphql_importer.rs` |
| HTTP Wrapper | `ccos/src/synthesis/importers/http_wrapper.rs` |
| Auth Injector | `ccos/src/synthesis/introspection/auth_injector.rs` |
| MCP Introspector | `ccos/src/synthesis/introspection/mcp_introspector.rs` |
| API Introspector | `ccos/src/synthesis/introspection/api_introspector.rs` |
| Capability Synthesizer | `ccos/src/synthesis/dialogue/capability_synthesizer.rs` |
| Schema Serializer | `ccos/src/synthesis/core/schema_serializer.rs` |
| A2A Discovery | `ccos/src/capability_marketplace/a2a_discovery.rs` |

---

## 12. See Also

- [030-capability-system-architecture.md](./030-capability-system-architecture.md) - Overall capability system
- [031-mcp-discovery-unified-service.md](./031-mcp-discovery-unified-service.md) - MCP discovery details
- [032-missing-capability-resolution.md](./032-missing-capability-resolution.md) - Resolution system
