//! GitHub MCP Capability Implementation (moved to runtime::capabilities::providers)
//!
//! Provides GitHub issue management via MCP protocol.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::time::Duration;

use crate::runtime::{RuntimeError, RuntimeResult, Value as RuntimeValue};
use crate::runtime::capabilities::provider::{
	CapabilityProvider, CapabilityDescriptor, SecurityRequirements,
	NetworkAccess, ResourceLimits, HealthStatus, ProviderConfig, ProviderMetadata,
	ExecutionContext
};
use crate::ast::{TypeExpr, PrimitiveType, MapKey};

/// GitHub MCP Server implementation
/// Provides GitHub issue management tools following MCP protocol standards
#[derive(Debug, Clone)]
pub struct GitHubMCPCapability {
	/// GitHub API token for authentication
	api_token: Option<String>,
	/// Base URL for GitHub API
	base_url: String,
	/// Default repository (owner/repo format)
	default_repo: Option<String>,
	/// Cache for recent API calls
	cache: HashMap<String, CachedGitHubData>,
}

/// Cached GitHub data to reduce API calls
#[derive(Debug, Clone)]
struct CachedGitHubData {
	data: Value,
	timestamp: std::time::SystemTime,
}

/// GitHub Issue structure
#[derive(Debug, Serialize, Deserialize)]
pub struct GitHubIssue {
	pub number: u64,
	pub title: String,
	pub body: Option<String>,
	pub state: String,
	pub labels: Vec<GitHubLabel>,
	pub assignees: Vec<GitHubUser>,
	pub created_at: String,
	pub updated_at: String,
	pub closed_at: Option<String>,
}

/// GitHub Label structure
#[derive(Debug, Serialize, Deserialize)]
pub struct GitHubLabel {
	pub name: String,
	pub color: String,
	pub description: Option<String>,
}

/// GitHub User structure
#[derive(Debug, Serialize, Deserialize)]
pub struct GitHubUser {
	pub login: String,
	pub id: u64,
	pub avatar_url: String,
}

/// MCP Tool definition for GitHub operations
#[derive(Debug, Serialize, Deserialize)]
pub struct MCPTool {
	name: String,
	description: String,
	input_schema: Value,
	output_schema: Option<Value>,
}

impl MCPTool {
	/// Create a tool for closing GitHub issues
	pub fn close_issue() -> Self {
		Self {
			name: "close_issue".to_string(),
			description: "Close a GitHub issue by number".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"owner": {"type": "string", "description": "Repository owner"},
					"repo": {"type": "string", "description": "Repository name"},
					"issue_number": {"type": "number", "description": "Issue number to close"},
					"comment": {"type": "string", "description": "Optional closing comment"}
				},
				"required": ["owner", "repo", "issue_number"]
			}),
			output_schema: Some(json!({
				"type": "object",
				"properties": {
					"success": {"type": "boolean"},
					"issue": {"type": "object"},
					"message": {"type": "string"}
				}
			})),
		}
	}

	/// Create a tool for creating GitHub issues
	pub fn create_issue() -> Self {
		Self {
			name: "create_issue".to_string(),
			description: "Create a new GitHub issue".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"owner": {"type": "string", "description": "Repository owner"},
					"repo": {"type": "string", "description": "Repository name"},
					"title": {"type": "string", "description": "Issue title"},
					"body": {"type": "string", "description": "Issue body/description"},
					"labels": {"type": "array", "items": {"type": "string"}},
					"assignees": {"type": "array", "items": {"type": "string"}}
				},
				"required": ["owner", "repo", "title"]
			}),
			output_schema: Some(json!({
				"type": "object",
				"properties": {
					"success": {"type": "boolean"},
					"issue": {"type": "object"},
					"issue_number": {"type": "number"},
					"html_url": {"type": "string"}
				}
			})),
		}
	}

	/// Create a tool for listing GitHub issues
	pub fn list_issues() -> Self {
		Self {
			name: "list_issues".to_string(),
			description: "List GitHub issues for a repository".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"owner": {"type": "string", "description": "Repository owner"},
					"repo": {"type": "string", "description": "Repository name"},
					"state": {"type": "string", "enum": ["open", "closed", "all"], "default": "open"},
					"per_page": {"type": "number", "default": 30},
					"page": {"type": "number", "default": 1}
				},
				"required": ["owner", "repo"]
			}),
			output_schema: Some(json!({
				"type": "object",
				"properties": {
					"success": {"type": "boolean"},
					"issues": {"type": "array", "items": {"type": "object"}},
					"total_count": {"type": "number"}
				}
			})),
		}
	}
}

impl GitHubMCPCapability {
	/// Create a new GitHub MCP capability
	pub fn new(api_token: Option<String>) -> Self {
		Self {
			api_token,
			base_url: "https://api.github.com".to_string(),
			default_repo: None,
			cache: HashMap::new(),
		}
	}

	/// Create a new GitHub MCP capability with default repository
	pub fn with_default_repo(api_token: Option<String>, owner: String, repo: String) -> Self {
		Self {
			api_token,
			base_url: "https://api.github.com".to_string(),
			default_repo: Some(format!("{}/{}", owner, repo)),
			cache: HashMap::new(),
		}
	}

	/// Close a GitHub issue
	pub async fn close_issue(&self, owner: &str, repo: &str, issue_number: u64, comment: Option<&str>) -> RuntimeResult<Value> {
		let client = reqwest::Client::new();
        
		// Prepare the request body
		let mut body = json!({
			"state": "closed"
		});

		// Add comment if provided
		if let Some(comment_text) = comment {
			body = json!({
				"state": "closed",
				"body": comment_text
			});
		}

		// Build the request
		let url = format!("{}/repos/{}/{}/issues/{}", self.base_url, owner, repo, issue_number);
		let mut request = client.patch(&url).json(&body);

		// Add authentication if available
		if let Some(token) = &self.api_token {
			request = request.header("Authorization", format!("token {}", token));
		}

		// Add required GitHub API headers
		request = request
			.header("Accept", "application/vnd.github.v3+json")
			.header("User-Agent", "CCOS-GitHub-MCP");

		// Send the request
		let response = request.send().await
			.map_err(|e| RuntimeError::Generic(format!("Failed to close GitHub issue: {}", e)))?;

		if response.status().is_success() {
			let issue: GitHubIssue = response.json().await
				.map_err(|e| RuntimeError::Generic(format!("Failed to parse GitHub response: {}", e)))?;

			Ok(json!({
				"success": true,
				"issue": issue,
				"message": format!("Issue #{} closed successfully", issue_number)
			}))
		} else {
			let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
			Err(RuntimeError::Generic(format!("Failed to close issue: {}", error_text)))
		}
	}

	/// Create a new GitHub issue
	pub async fn create_issue(&self, owner: &str, repo: &str, title: &str, body: Option<&str>, labels: Option<Vec<String>>, assignees: Option<Vec<String>>) -> RuntimeResult<Value> {
		let client = reqwest::Client::new();
        
		// Prepare the request body
		let mut body_json = json!({
			"title": title
		});

		if let Some(body_text) = body {
			body_json["body"] = json!(body_text);
		}

		if let Some(labels_vec) = labels {
			body_json["labels"] = json!(labels_vec);
		}

		if let Some(assignees_vec) = assignees {
			body_json["assignees"] = json!(assignees_vec);
		}

		// Build the request
		let url = format!("{}/repos/{}/{}/issues", self.base_url, owner, repo);
		let mut request = client.post(&url).json(&body_json);

		// Add authentication if available
		if let Some(token) = &self.api_token {
			request = request.header("Authorization", format!("token {}", token));
		}

		// Add required GitHub API headers
		request = request
			.header("Accept", "application/vnd.github.v3+json")
			.header("User-Agent", "CCOS-GitHub-MCP");

		// Send the request
		let response = request.send().await
			.map_err(|e| RuntimeError::Generic(format!("Failed to create GitHub issue: {}", e)))?;

		if response.status().is_success() {
			let issue: GitHubIssue = response.json().await
				.map_err(|e| RuntimeError::Generic(format!("Failed to parse GitHub response: {}", e)))?;

			Ok(json!({
				"success": true,
				"issue": issue,
				"issue_number": issue.number,
				"html_url": format!("https://github.com/{}/{}/issues/{}", owner, repo, issue.number)
			}))
		} else {
			let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
			Err(RuntimeError::Generic(format!("Failed to create issue: {}", error_text)))
		}
	}

	/// List GitHub issues
	pub async fn list_issues(&self, owner: &str, repo: &str, state: Option<&str>, per_page: Option<u32>, page: Option<u32>) -> RuntimeResult<Value> {
		let client = reqwest::Client::new();
        
		// Build the URL with query parameters
		let mut url = format!("{}/repos/{}/{}/issues", self.base_url, owner, repo);
		let mut query_params = Vec::new();
        
		if let Some(state_val) = state {
			query_params.push(format!("state={}", state_val));
		}
        
		if let Some(per_page_val) = per_page {
			query_params.push(format!("per_page={}", per_page_val));
		}
        
		if let Some(page_val) = page {
			query_params.push(format!("page={}", page_val));
		}
        
		if !query_params.is_empty() {
			url.push_str(&format!("?{}", query_params.join("&")));
		}

		// Build the request
		let mut request = client.get(&url);

		// Add authentication if available
		if let Some(token) = &self.api_token {
			request = request.header("Authorization", format!("token {}", token));
		}

		// Add required GitHub API headers
		request = request
			.header("Accept", "application/vnd.github.v3+json")
			.header("User-Agent", "CCOS-GitHub-MCP");

		// Send the request
		let response = request.send().await
			.map_err(|e| RuntimeError::Generic(format!("Failed to list GitHub issues: {}", e)))?;

		if response.status().is_success() {
			let issues: Vec<GitHubIssue> = response.json().await
				.map_err(|e| RuntimeError::Generic(format!("Failed to parse GitHub response: {}", e)))?;

			Ok(json!({
				"success": true,
				"issues": issues,
				"total_count": issues.len()
			}))
		} else {
			let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
			Err(RuntimeError::Generic(format!("Failed to list issues: {}", error_text)))
		}
	}

	/// Get available tools for this MCP server
	pub fn get_tools(&self) -> Vec<MCPTool> {
		vec![
			MCPTool::close_issue(),
			MCPTool::create_issue(),
			MCPTool::list_issues(),
		]
	}

	/// Execute a tool by name
	pub async fn execute_tool(&self, tool_name: &str, arguments: &Value) -> RuntimeResult<Value> {
		match tool_name {
			"close_issue" => {
				let owner = arguments["owner"].as_str()
					.ok_or_else(|| RuntimeError::Generic("Missing 'owner' parameter".to_string()))?;
				let repo = arguments["repo"].as_str()
					.ok_or_else(|| RuntimeError::Generic("Missing 'repo' parameter".to_string()))?;
				let issue_number = arguments["issue_number"].as_u64()
					.ok_or_else(|| RuntimeError::Generic("Missing or invalid 'issue_number' parameter".to_string()))?;
				let comment = arguments["comment"].as_str();

				self.close_issue(owner, repo, issue_number, comment).await
			}
			"create_issue" => {
				let owner = arguments["owner"].as_str()
					.ok_or_else(|| RuntimeError::Generic("Missing 'owner' parameter".to_string()))?;
				let repo = arguments["repo"].as_str()
					.ok_or_else(|| RuntimeError::Generic("Missing 'repo' parameter".to_string()))?;
				let title = arguments["title"].as_str()
					.ok_or_else(|| RuntimeError::Generic("Missing 'title' parameter".to_string()))?;
				let body = arguments["body"].as_str();
                
				let labels = if let Some(labels_array) = arguments["labels"].as_array() {
					Some(labels_array.iter()
						.filter_map(|v| v.as_str().map(|s| s.to_string()))
						.collect())
				} else {
					None
				};
                
				let assignees = if let Some(assignees_array) = arguments["assignees"].as_array() {
					Some(assignees_array.iter()
						.filter_map(|v| v.as_str().map(|s| s.to_string()))
						.collect())
				} else {
					None
				};

				self.create_issue(owner, repo, title, body, labels, assignees).await
			}
			"list_issues" => {
				let owner = arguments["owner"].as_str()
					.ok_or_else(|| RuntimeError::Generic("Missing 'owner' parameter".to_string()))?;
				let repo = arguments["repo"].as_str()
					.ok_or_else(|| RuntimeError::Generic("Missing 'repo' parameter".to_string()))?;
				let state = arguments["state"].as_str();
				let per_page = arguments["per_page"].as_u64().map(|v| v as u32);
				let page = arguments["page"].as_u64().map(|v| v as u32);

				self.list_issues(owner, repo, state, per_page, page).await
			}
			_ => Err(RuntimeError::Generic(format!("Unknown tool: {}", tool_name))),
		}
	}
}

impl CapabilityProvider for GitHubMCPCapability {
	fn provider_id(&self) -> &str {
		"github_mcp"
	}

	fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
		vec![
			CapabilityDescriptor {
				id: "github.close_issue".to_string(),
				description: "Close GitHub issues via MCP".to_string(),
				capability_type: TypeExpr::Primitive(PrimitiveType::String),
				security_requirements: SecurityRequirements {
					permissions: vec![],
					requires_microvm: false,
					resource_limits: ResourceLimits {
						max_memory: Some(64),
						max_cpu_time: Some(Duration::from_secs(30)),
						max_disk_space: None,
					},
					network_access: NetworkAccess::Limited(vec!["api.github.com".to_string()]),
				},
				metadata: HashMap::new(),
			},
			CapabilityDescriptor {
				id: "github.create_issue".to_string(),
				description: "Create GitHub issues via MCP".to_string(),
				capability_type: TypeExpr::Primitive(PrimitiveType::String),
				security_requirements: SecurityRequirements {
					permissions: vec![],
					requires_microvm: false,
					resource_limits: ResourceLimits {
						max_memory: Some(64),
						max_cpu_time: Some(Duration::from_secs(30)),
						max_disk_space: None,
					},
					network_access: NetworkAccess::Limited(vec!["api.github.com".to_string()]),
				},
				metadata: HashMap::new(),
			},
			CapabilityDescriptor {
				id: "github.list_issues".to_string(),
				description: "List GitHub issues via MCP".to_string(),
				capability_type: TypeExpr::Primitive(PrimitiveType::String),
				security_requirements: SecurityRequirements {
					permissions: vec![],
					requires_microvm: false,
					resource_limits: ResourceLimits {
						max_memory: Some(64),
						max_cpu_time: Some(Duration::from_secs(30)),
						max_disk_space: None,
					},
					network_access: NetworkAccess::Limited(vec!["api.github.com".to_string()]),
				},
				metadata: HashMap::new(),
			},
		]
	}

	fn execute_capability(
		&self,
		capability_id: &str,
		inputs: &RuntimeValue,
		_context: &ExecutionContext,
	) -> RuntimeResult<RuntimeValue> {
		// Convert RTFS Value to JSON Value
		let json_inputs = match inputs {
			RuntimeValue::Map(map) => {
				let mut json_map = serde_json::Map::new();
				for (k, v) in map {
					let key = match k {
						MapKey::String(s) => s.clone(),
						MapKey::Keyword(s) => s.0.clone(),
						MapKey::Integer(i) => i.to_string(),
					};
					json_map.insert(key, self.runtime_value_to_json(v)?);
				}
				Value::Object(json_map)
			}
			_ => return Err(RuntimeError::Generic("Expected map input for GitHub MCP capability".to_string())),
		};

		// Extract tool name and arguments
		let tool_name = json_inputs["tool"].as_str()
			.ok_or_else(|| RuntimeError::Generic("Missing 'tool' parameter".to_string()))?;
		let arguments = &json_inputs["arguments"];

		// Execute the tool
		let result = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.execute_tool(tool_name, arguments))?;

		// Convert JSON result back to RTFS Value
		self.json_to_runtime_value(&result)
	}

	fn initialize(&mut self, _config: &ProviderConfig) -> Result<(), String> {
		Ok(())
	}

	fn health_check(&self) -> HealthStatus {
		HealthStatus::Healthy
	}

	fn metadata(&self) -> ProviderMetadata {
		ProviderMetadata {
			name: "GitHub MCP Server".to_string(),
			version: "1.0.0".to_string(),
			description: "GitHub issue management via MCP protocol".to_string(),
			author: "CCOS Team".to_string(),
			license: Some("MIT".to_string()),
			dependencies: vec![],
		}
	}
}

impl GitHubMCPCapability {
	/// Convert RTFS RuntimeValue to JSON Value
	fn runtime_value_to_json(&self, value: &RuntimeValue) -> RuntimeResult<Value> {
		match value {
			RuntimeValue::Nil => Ok(Value::Null),
			// Atoms are mutable refs; serialize as a tagged string for now
            // RuntimeValue::Atom variant removed - no longer exists
			RuntimeValue::String(s) => Ok(Value::String(s.clone())),
			RuntimeValue::Float(n) => {
				if n.fract() == 0.0 {
					Ok(Value::Number(serde_json::Number::from(*n as i64)))
				} else {
					Ok(Value::Number(serde_json::Number::from_f64(*n).unwrap_or_else(|| serde_json::Number::from(0))))
				}
			}
			RuntimeValue::Integer(n) => Ok(Value::Number(serde_json::Number::from(*n))),
			RuntimeValue::Boolean(b) => Ok(Value::Bool(*b)),
			RuntimeValue::Timestamp(t) => Ok(Value::String(t.clone())),
			RuntimeValue::Uuid(u) => Ok(Value::String(u.clone())),
			RuntimeValue::ResourceHandle(r) => Ok(Value::String(r.clone())),
			RuntimeValue::Symbol(s) => Ok(Value::String(s.0.clone())),
			RuntimeValue::Keyword(k) => Ok(Value::String(k.0.clone())),
			RuntimeValue::Vector(v) => {
				let mut json_vec = Vec::new();
				for item in v {
					json_vec.push(self.runtime_value_to_json(item)?);
				}
				Ok(Value::Array(json_vec))
			}
			RuntimeValue::List(l) => {
				let mut json_vec = Vec::new();
				for item in l {
					json_vec.push(self.runtime_value_to_json(item)?);
				}
				Ok(Value::Array(json_vec))
			}
			RuntimeValue::Map(m) => {
				let mut json_map = serde_json::Map::new();
				for (k, v) in m {
					let key = match k {
						MapKey::String(s) => s.clone(),
						MapKey::Keyword(s) => s.0.clone(),
						MapKey::Integer(i) => i.to_string(),
					};
					json_map.insert(key, self.runtime_value_to_json(v)?);
				}
				Ok(Value::Object(json_map))
			}
			RuntimeValue::Function(_) => Ok(Value::String("#<function>".to_string())),
			RuntimeValue::FunctionPlaceholder(_) => Ok(Value::String("#<function-placeholder>".to_string())),
			RuntimeValue::Error(e) => Ok(Value::String(format!("#<error: {}>", e.message))),
		}
	}

	/// Convert JSON Value to RTFS RuntimeValue
	fn json_to_runtime_value(&self, value: &Value) -> RuntimeResult<RuntimeValue> {
		match value {
			Value::String(s) => Ok(RuntimeValue::String(s.clone())),
			Value::Number(n) => {
				if let Some(i) = n.as_i64() {
					Ok(RuntimeValue::Integer(i))
				} else if let Some(f) = n.as_f64() {
					Ok(RuntimeValue::Float(f))
				} else {
					Ok(RuntimeValue::Integer(0))
				}
			}
			Value::Bool(b) => Ok(RuntimeValue::Boolean(*b)),
			Value::Array(a) => {
				let mut runtime_vec = Vec::new();
				for item in a {
					runtime_vec.push(self.json_to_runtime_value(item)?);
				}
				Ok(RuntimeValue::Vector(runtime_vec))
			}
			Value::Object(o) => {
				let mut runtime_map = HashMap::new();
				for (k, v) in o {
					runtime_map.insert(MapKey::String(k.clone()), self.json_to_runtime_value(v)?);
				}
				Ok(RuntimeValue::Map(runtime_map))
			}
			Value::Null => Ok(RuntimeValue::Nil),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_github_mcp_capability_creation() {
		let capability = GitHubMCPCapability::new(Some("test_token".to_string()));
		assert_eq!(capability.base_url, "https://api.github.com");
		assert_eq!(capability.api_token, Some("test_token".to_string()));
	}

	#[tokio::test]
	async fn test_github_mcp_capability_with_default_repo() {
		let capability = GitHubMCPCapability::with_default_repo(
			Some("test_token".to_string()),
			"mandubian".to_string(),
			"ccos".to_string()
		);
		assert_eq!(capability.default_repo, Some("mandubian/ccos".to_string()));
	}

	#[tokio::test]
	async fn test_mcp_tool_creation() {
		let close_tool = MCPTool::close_issue();
		assert_eq!(close_tool.name, "close_issue");
		assert!(close_tool.description.contains("Close"));

		let create_tool = MCPTool::create_issue();
		assert_eq!(create_tool.name, "create_issue");
		assert!(create_tool.description.contains("Create"));
	}

	#[tokio::test]
	async fn test_get_tools() {
		let capability = GitHubMCPCapability::new(None);
		let tools = capability.get_tools();
		assert_eq!(tools.len(), 3);
        
		let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
		assert!(tool_names.contains(&"close_issue".to_string()));
		assert!(tool_names.contains(&"create_issue".to_string()));
		assert!(tool_names.contains(&"list_issues".to_string()));
	}

	#[tokio::test]
	async fn test_value_conversion() {
		let capability = GitHubMCPCapability::new(None);
        
		// Test RTFS to JSON conversion
		let rtfs_value = RuntimeValue::Map({
			let mut map = HashMap::new();
			map.insert(MapKey::String("test".to_string()), RuntimeValue::String("value".to_string()));
			map.insert(MapKey::String("number".to_string()), RuntimeValue::Integer(42));
			map
		});
        
		let json_value = capability.runtime_value_to_json(&rtfs_value).unwrap();
		assert!(json_value.is_object());
        
		// Test JSON to RTFS conversion
		let converted_back = capability.json_to_runtime_value(&json_value).unwrap();
		assert!(matches!(converted_back, RuntimeValue::Map(_)));
	}

	#[tokio::test]
	async fn test_capability_provider_implementation() {
		let capability = GitHubMCPCapability::new(None);
        
		assert_eq!(capability.provider_id(), "github_mcp");
		assert_eq!(capability.health_check(), HealthStatus::Healthy);
        
		let metadata = capability.metadata();
		assert_eq!(metadata.name, "GitHub MCP Server");
		assert_eq!(metadata.version, "1.0.0");
        
		let capabilities = capability.list_capabilities();
		assert_eq!(capabilities.len(), 3);
	}
}
