use crate::capability_marketplace::types::CapabilityManifest;
use crate::synthesis::auth_injector::AuthInjector;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// GraphQL Operation (query/mutation/subscription) information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLOperation {
    /// Operation type (query, mutation, subscription)
    pub operation_type: String,
    /// Operation name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Input parameters/arguments
    pub arguments: Vec<GraphQLArgument>,
    /// Return type
    pub return_type: String,
    /// Whether auth is required
    pub requires_auth: bool,
}

/// GraphQL Argument (input parameter) information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLArgument {
    /// Argument name
    pub name: String,
    /// GraphQL type (String!, Int, Boolean, etc.)
    pub gql_type: String,
    /// Whether argument is required (non-null)
    pub required: bool,
    /// Default value if any
    pub default_value: Option<serde_json::Value>,
    /// Description
    pub description: Option<String>,
}

/// GraphQL Schema information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLSchema {
    /// GraphQL schema content
    pub schema_content: serde_json::Value,
    /// Endpoint URL
    pub endpoint_url: String,
    /// Introspection query result
    pub introspection_result: Option<serde_json::Value>,
}

/// GraphQL Importer for converting GraphQL schemas to CCOS capabilities
pub struct GraphQLImporter {
    /// GraphQL endpoint URL
    pub endpoint_url: String,
    /// Auth injector for handling credentials
    auth_injector: AuthInjector,
    /// Mock mode for testing
    mock_mode: bool,
}

impl GraphQLImporter {
    /// Create a new GraphQL importer
    pub fn new(endpoint_url: String) -> Self {
        Self {
            endpoint_url,
            auth_injector: AuthInjector::new(),
            mock_mode: false,
        }
    }

    /// Create in mock mode for testing
    pub fn mock(endpoint_url: String) -> Self {
        Self {
            endpoint_url,
            auth_injector: AuthInjector::mock(),
            mock_mode: true,
        }
    }

    /// Perform GraphQL introspection to get schema
    pub async fn introspect_schema(&self) -> RuntimeResult<GraphQLSchema> {
        if self.mock_mode {
            return self.get_mock_schema();
        }

        let introspection_query = r#"
        query IntrospectionQuery {
            __schema {
                queryType { name }
                mutationType { name }
                subscriptionType { name }
                types {
                    ...FullType
                }
            }
        }
        
        fragment FullType on __Type {
            kind
            name
            description
            fields(includeDeprecated: true) {
                name
                description
                args {
                    ...InputValue
                }
                type {
                    ...TypeRef
                }
                isDeprecated
                deprecationReason
            }
            inputFields {
                ...InputValue
            }
            interfaces {
                ...TypeRef
            }
            enumValues(includeDeprecated: true) {
                name
                description
                isDeprecated
                deprecationReason
            }
            possibleTypes {
                ...TypeRef
            }
        }
        
        fragment InputValue on __InputValue {
            name
            description
            type { ...TypeRef }
            defaultValue
        }
        
        fragment TypeRef on __Type {
            kind
            name
            ofType {
                kind
                name
                ofType {
                    kind
                    name
                    ofType {
                        kind
                        name
                        ofType {
                            kind
                            name
                            ofType {
                                kind
                                name
                                ofType {
                                    kind
                                    name
                                    ofType {
                                        kind
                                        name
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "#;

        eprintln!(
            "📥 Introspecting GraphQL schema from: {}",
            self.endpoint_url
        );

        // In real implementation: make HTTP POST request to GraphQL endpoint
        // For now, return placeholder error
        Err(RuntimeError::Generic(
            "GraphQL introspection not yet implemented - requires HTTP client".to_string(),
        ))
    }

    /// Parse GraphQL schema and extract operations
    pub fn extract_operations(
        &self,
        schema: &GraphQLSchema,
    ) -> RuntimeResult<Vec<GraphQLOperation>> {
        let introspection = schema.introspection_result.as_ref().ok_or_else(|| {
            RuntimeError::Generic("No introspection result available".to_string())
        })?;

        let schema_data = introspection
            .get("data")
            .and_then(|d| d.get("__schema"))
            .ok_or_else(|| {
                RuntimeError::Generic("Invalid GraphQL introspection result".to_string())
            })?;

        let mut operations = Vec::new();

        // Extract queries
        if let Some(query_type) = schema_data.get("queryType") {
            if let Some(query_name) = query_type.get("name").and_then(|n| n.as_str()) {
                if let Some(query_fields) = self.get_type_fields(schema_data, query_name)? {
                    for field in query_fields {
                        let operation = self.parse_field_as_operation("query", &field)?;
                        operations.push(operation);
                    }
                }
            }
        }

        // Extract mutations
        if let Some(mutation_type) = schema_data.get("mutationType") {
            if let Some(mutation_name) = mutation_type.get("name").and_then(|n| n.as_str()) {
                if let Some(mutation_fields) = self.get_type_fields(schema_data, mutation_name)? {
                    for field in mutation_fields {
                        let operation = self.parse_field_as_operation("mutation", &field)?;
                        operations.push(operation);
                    }
                }
            }
        }

        // Extract subscriptions
        if let Some(subscription_type) = schema_data.get("subscriptionType") {
            if let Some(subscription_name) = subscription_type.get("name").and_then(|n| n.as_str())
            {
                if let Some(subscription_fields) =
                    self.get_type_fields(schema_data, subscription_name)?
                {
                    for field in subscription_fields {
                        let operation = self.parse_field_as_operation("subscription", &field)?;
                        operations.push(operation);
                    }
                }
            }
        }

        Ok(operations)
    }

    /// Get fields for a specific type
    fn get_type_fields(
        &self,
        schema_data: &serde_json::Value,
        type_name: &str,
    ) -> RuntimeResult<Option<Vec<serde_json::Value>>> {
        let types = schema_data
            .get("types")
            .and_then(|t| t.as_array())
            .ok_or_else(|| RuntimeError::Generic("No types found in schema".to_string()))?;

        for type_def in types {
            if let Some(name) = type_def.get("name").and_then(|n| n.as_str()) {
                if name == type_name {
                    return Ok(type_def.get("fields").and_then(|f| f.as_array()).cloned());
                }
            }
        }

        Ok(None)
    }

    /// Parse a GraphQL field as an operation
    fn parse_field_as_operation(
        &self,
        operation_type: &str,
        field: &serde_json::Value,
    ) -> RuntimeResult<GraphQLOperation> {
        let name = field
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| RuntimeError::Generic("Field missing name".to_string()))?
            .to_string();

        let description = field
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let mut arguments = Vec::new();
        if let Some(args) = field.get("args").and_then(|a| a.as_array()) {
            for arg in args {
                arguments.push(self.parse_argument(arg)?);
            }
        }

        let return_type = self.extract_return_type(field)?;
        let requires_auth = self.detect_auth_requirement(field);

        Ok(GraphQLOperation {
            operation_type: operation_type.to_string(),
            name,
            description,
            arguments,
            return_type,
            requires_auth,
        })
    }

    /// Parse a GraphQL argument
    fn parse_argument(&self, arg: &serde_json::Value) -> RuntimeResult<GraphQLArgument> {
        let name = arg
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| RuntimeError::Generic("Argument missing name".to_string()))?
            .to_string();

        let description = arg
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let type_info = arg
            .get("type")
            .ok_or_else(|| RuntimeError::Generic("Argument missing type".to_string()))?;

        let (gql_type, required) = self.parse_graphql_type(type_info)?;
        let default_value = arg.get("defaultValue").cloned();

        Ok(GraphQLArgument {
            name,
            gql_type,
            required,
            default_value,
            description,
        })
    }

    /// Parse GraphQL type and determine if it's required
    fn parse_graphql_type(&self, type_info: &serde_json::Value) -> RuntimeResult<(String, bool)> {
        // Handle nested type references (GraphQL uses nested structure for non-null types)
        let mut current = type_info;
        let mut required = false;

        while let Some(of_type) = current.get("ofType") {
            if current.get("kind").and_then(|k| k.as_str()) == Some("NON_NULL") {
                required = true;
            }
            current = of_type;
        }

        let type_name = current
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown");

        Ok((type_name.to_string(), required))
    }

    /// Extract return type from field
    fn extract_return_type(&self, field: &serde_json::Value) -> RuntimeResult<String> {
        if let Some(type_info) = field.get("type") {
            let (type_name, _) = self.parse_graphql_type(type_info)?;
            Ok(type_name)
        } else {
            Ok("Unknown".to_string())
        }
    }

    /// Detect if operation requires authentication
    fn detect_auth_requirement(&self, field: &serde_json::Value) -> bool {
        // Simple heuristic: check if field name suggests auth requirement
        let name = field.get("name").and_then(|n| n.as_str()).unwrap_or("");

        let auth_keywords = [
            "user",
            "profile",
            "account",
            "settings",
            "create",
            "update",
            "delete",
            "publish",
            "unpublish",
            "admin",
            "private",
            "secret",
        ];

        auth_keywords
            .iter()
            .any(|keyword| name.to_lowercase().contains(keyword))
    }

    /// Convert GraphQL operation to CCOS capability
    pub fn operation_to_capability(
        &self,
        operation: &GraphQLOperation,
        api_name: &str,
    ) -> RuntimeResult<CapabilityManifest> {
        let capability_id = format!(
            "graphql.{}.{}.{}",
            api_name, operation.operation_type, operation.name
        );

        let description = operation.description.clone().unwrap_or_else(|| {
            format!(
                "GraphQL {} operation: {}",
                operation.operation_type, operation.name
            )
        });

        // Build parameters map
        let mut parameters_map = HashMap::new();
        for arg in &operation.arguments {
            let param_type = self.graphql_type_to_rtfs_type(&arg.gql_type);
            parameters_map.insert(arg.name.clone(), param_type);
        }

        // Add auth_token parameter if auth is required
        let mut effects = vec![":network".to_string()];
        if operation.requires_auth {
            effects.push(":auth".to_string());
            if parameters_map.get("auth_token").is_none() {
                parameters_map.insert("auth_token".to_string(), ":string".to_string());
            }
        }

        // Build metadata
        let mut metadata = HashMap::new();
        metadata.insert(
            "graphql_operation_type".to_string(),
            operation.operation_type.clone(),
        );
        metadata.insert("graphql_operation_name".to_string(), operation.name.clone());
        metadata.insert("graphql_endpoint".to_string(), self.endpoint_url.clone());
        metadata.insert(
            "graphql_return_type".to_string(),
            operation.return_type.clone(),
        );
        if operation.requires_auth {
            metadata.insert("auth_required".to_string(), "true".to_string());
            metadata.insert("auth_providers".to_string(), "graphql".to_string());
        }

        Ok(CapabilityManifest {
            id: capability_id,
            name: operation.name.clone(),
            description,
            provider: crate::capability_marketplace::types::ProviderType::Local(
                crate::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(rtfs::runtime::values::Value::String(
                            "GraphQL operation placeholder".to_string(),
                        ))
                    }),
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "graphql_importer".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("graphql_{}_{}", operation.operation_type, operation.name),
                custody_chain: vec!["graphql_importer".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            effects,
            metadata,
            agent_metadata: None,
        })
    }

    /// Convert GraphQL type to RTFS keyword type
    pub fn graphql_type_to_rtfs_type(&self, gql_type: &str) -> String {
        match gql_type.to_lowercase().as_str() {
            "string" => ":string".to_string(),
            "int" | "integer" => ":number".to_string(),
            "float" => ":number".to_string(),
            "boolean" => ":boolean".to_string(),
            "id" => ":string".to_string(),
            "date" | "datetime" => ":string".to_string(),
            _ => ":any".to_string(),
        }
    }

    /// Generate GraphQL query/mutation code for capability
    pub fn generate_graphql_code(&self, operation: &GraphQLOperation) -> RuntimeResult<String> {
        let mut query = String::new();

        // Build arguments string
        let mut args_str = String::new();
        if !operation.arguments.is_empty() {
            args_str.push('(');
            let arg_parts: Vec<String> = operation
                .arguments
                .iter()
                .map(|arg| {
                    let type_str = if arg.required {
                        format!(
                            "{}: {}",
                            arg.name,
                            self.graphql_type_to_rtfs_type(&arg.gql_type)
                        )
                    } else {
                        format!(
                            "{}: {}?",
                            arg.name,
                            self.graphql_type_to_rtfs_type(&arg.gql_type)
                        )
                    };
                    type_str
                })
                .collect();
            args_str.push_str(&arg_parts.join(", "));
            args_str.push(')');
        }

        // Build query/mutation
        match operation.operation_type.as_str() {
            "query" => {
                query.push_str(&format!("query {}{} {{\n", operation.name, args_str));
                query.push_str(&format!("  {}\n", operation.name));
                if !operation.arguments.is_empty() {
                    let arg_vars: Vec<String> = operation
                        .arguments
                        .iter()
                        .map(|arg| format!("{}: ${}", arg.name, arg.name))
                        .collect();
                    query.push_str(&format!("({})", arg_vars.join(", ")));
                }
                query.push_str(" {\n    # Add fields here\n  }\n}");
            }
            "mutation" => {
                query.push_str(&format!("mutation {}{} {{\n", operation.name, args_str));
                query.push_str(&format!("  {}\n", operation.name));
                if !operation.arguments.is_empty() {
                    let arg_vars: Vec<String> = operation
                        .arguments
                        .iter()
                        .map(|arg| format!("{}: ${}", arg.name, arg.name))
                        .collect();
                    query.push_str(&format!("(input: {{{}}})", arg_vars.join(", ")));
                }
                query.push_str(" {\n    # Add return fields here\n  }\n}");
            }
            "subscription" => {
                query.push_str(&format!("subscription {}{} {{\n", operation.name, args_str));
                query.push_str(&format!("  {}\n", operation.name));
                query.push_str(" {\n    # Add subscription fields here\n  }\n}");
            }
            _ => {
                return Err(RuntimeError::Generic(format!(
                    "Unknown operation type: {}",
                    operation.operation_type
                )));
            }
        }

        Ok(query)
    }

    /// Get mock GraphQL schema for testing
    fn get_mock_schema(&self) -> RuntimeResult<GraphQLSchema> {
        let mock_introspection = serde_json::json!({
            "data": {
                "__schema": {
                    "queryType": {"name": "Query"},
                    "mutationType": {"name": "Mutation"},
                    "types": [
                        {
                            "kind": "OBJECT",
                            "name": "Query",
                            "fields": [
                                {
                                    "name": "user",
                                    "description": "Get current user",
                                    "args": [
                                        {
                                            "name": "id",
                                            "type": {
                                                "kind": "NON_NULL",
                                                "name": null,
                                                "ofType": {"kind": "SCALAR", "name": "ID"}
                                            }
                                        }
                                    ],
                                    "type": {"kind": "OBJECT", "name": "User"}
                                }
                            ]
                        },
                        {
                            "kind": "OBJECT",
                            "name": "Mutation",
                            "fields": [
                                {
                                    "name": "createPost",
                                    "description": "Create a new post",
                                    "args": [
                                        {
                                            "name": "title",
                                            "type": {
                                                "kind": "NON_NULL",
                                                "name": null,
                                                "ofType": {"kind": "SCALAR", "name": "String"}
                                            }
                                        },
                                        {
                                            "name": "content",
                                            "type": {
                                                "kind": "NON_NULL",
                                                "name": null,
                                                "ofType": {"kind": "SCALAR", "name": "String"}
                                            }
                                        }
                                    ],
                                    "type": {"kind": "OBJECT", "name": "Post"}
                                }
                            ]
                        }
                    ]
                }
            }
        });

        Ok(GraphQLSchema {
            schema_content: mock_introspection.clone(),
            endpoint_url: self.endpoint_url.clone(),
            introspection_result: Some(mock_introspection),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphql_importer_creation() {
        let importer = GraphQLImporter::new("https://api.example.com/graphql".to_string());
        assert_eq!(importer.endpoint_url, "https://api.example.com/graphql");
    }

    #[test]
    fn test_graphql_type_to_rtfs_type() {
        let importer = GraphQLImporter::mock("https://api.example.com/graphql".to_string());

        assert_eq!(importer.graphql_type_to_rtfs_type("String"), ":string");
        assert_eq!(importer.graphql_type_to_rtfs_type("Int"), ":number");
        assert_eq!(importer.graphql_type_to_rtfs_type("Boolean"), ":boolean");
        assert_eq!(importer.graphql_type_to_rtfs_type("ID"), ":string");
        assert_eq!(importer.graphql_type_to_rtfs_type("UnknownType"), ":any");
    }

    #[test]
    fn test_detect_auth_requirement() {
        let importer = GraphQLImporter::mock("https://api.example.com/graphql".to_string());

        let user_field = serde_json::json!({
            "name": "getUserProfile",
            "description": "Get user profile"
        });
        assert!(importer.detect_auth_requirement(&user_field));

        let public_field = serde_json::json!({
            "name": "getPublicData",
            "description": "Get public data"
        });
        assert!(!importer.detect_auth_requirement(&public_field));
    }

    #[tokio::test]
    async fn test_introspect_mock_schema() {
        let importer = GraphQLImporter::mock("https://api.example.com/graphql".to_string());
        let schema = importer.introspect_schema().await.unwrap();

        assert!(schema.introspection_result.is_some());
        assert_eq!(schema.endpoint_url, "https://api.example.com/graphql");
    }

    #[test]
    fn test_parse_graphql_type() {
        let importer = GraphQLImporter::mock("https://api.example.com/graphql".to_string());

        // Non-null type
        let non_null_type = serde_json::json!({
            "kind": "NON_NULL",
            "name": null,
            "ofType": {"kind": "SCALAR", "name": "String"}
        });
        let (type_name, required) = importer.parse_graphql_type(&non_null_type).unwrap();
        assert_eq!(type_name, "String");
        assert!(required);

        // Nullable type
        let nullable_type = serde_json::json!({
            "kind": "SCALAR",
            "name": "Int"
        });
        let (type_name, required) = importer.parse_graphql_type(&nullable_type).unwrap();
        assert_eq!(type_name, "Int");
        assert!(!required);
    }

    #[test]
    fn test_generate_graphql_code() {
        let importer = GraphQLImporter::mock("https://api.example.com/graphql".to_string());

        let operation = GraphQLOperation {
            operation_type: "query".to_string(),
            name: "getUser".to_string(),
            description: Some("Get user by ID".to_string()),
            arguments: vec![GraphQLArgument {
                name: "id".to_string(),
                gql_type: "ID".to_string(),
                required: true,
                default_value: None,
                description: Some("User ID".to_string()),
            }],
            return_type: "User".to_string(),
            requires_auth: false,
        };

        let query = importer.generate_graphql_code(&operation).unwrap();
        assert!(query.contains("query getUser"));
        assert!(query.contains("id:")); // The function generates RTFS format with converted types
    }
}
