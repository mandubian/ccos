/// Missing Capability Resolution Strategies
///
/// This module provides modular strategies for handling missing capabilities
/// when no existing capability is found through discovery. The strategies are
/// designed to be reusable, factorized, and generic to work with any goal,
/// intent, or plan in the CCOS system.
///
/// Strategies include:
/// - Pure RTFS generation
/// - User interaction for clarification
/// - External LLM hints
/// - Service discovery hints
use std::sync::Arc;

use async_trait::async_trait;

use super::missing_capability_resolver::{MissingCapabilityRequest, ResolutionResult};
use crate::arbiter::DelegatingArbiter;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::planner::modular_planner::types::{
    ApiAction, DomainHint, IntentType, OutputFormat, ToolSummary, TransformType,
};
use crate::planner::modular_planner::{ResolutionContext, ResolutionError};

/// Trait for missing capability resolution strategies
#[async_trait]
pub trait MissingCapabilityStrategy: Send + Sync {
    /// Get the name of this strategy
    fn name(&self) -> &str;

    /// Check if this strategy can handle the given missing capability request
    fn can_handle(&self, request: &MissingCapabilityRequest) -> bool;

    /// Attempt to resolve the missing capability using this strategy
    async fn resolve(
        &self,
        request: &MissingCapabilityRequest,
        context: &ResolutionContext,
    ) -> Result<ResolutionResult, ResolutionError>;

    /// Get a summary of available tools that this strategy can generate
    async fn list_available_tools(&self, domain_hints: Option<&[DomainHint]>) -> Vec<ToolSummary> {
        vec![]
    }

    /// Whether this strategy supports service-discovery hinting
    fn supports_service_discovery(&self) -> bool {
        false
    }

    /// Maximum number of attempts for each strategy
    fn max_attempts(&self) -> u32 {
        3
    }

    /// Timeout for external LLM calls (in milliseconds)
    fn llm_timeout_ms(&self) -> u64 {
        30_000
    }
}

/// Configuration for missing-capability strategies
#[derive(Debug, Clone)]
pub struct MissingCapabilityStrategyConfig {
    pub enable_pure_rtfs: bool,
    pub enable_user_interaction: bool,
    pub enable_external_llm: bool,
    pub enable_service_discovery: bool,
    pub max_attempts: u32,
    pub llm_timeout_ms: u64,
}

impl Default for MissingCapabilityStrategyConfig {
    fn default() -> Self {
        Self {
            enable_pure_rtfs: true,
            enable_user_interaction: true,
            enable_external_llm: true,
            enable_service_discovery: true,
            max_attempts: 3,
            llm_timeout_ms: 30_000,
        }
    }
}

/// Composite strategy that combines multiple missing capability strategies
pub struct CompositeMissingCapabilityStrategy {
    strategies: Vec<Arc<dyn MissingCapabilityStrategy>>,
    config: MissingCapabilityStrategyConfig,
}

impl CompositeMissingCapabilityStrategy {
    pub fn new(config: MissingCapabilityStrategyConfig) -> Self {
        Self {
            strategies: Vec::new(),
            config,
        }
    }

    /// Add a strategy to the composite
    pub fn add_strategy(&mut self, strategy: Arc<dyn MissingCapabilityStrategy>) {
        self.strategies.push(strategy);
    }

    /// Get all available tools from every strategy
    pub async fn get_all_available_tools(
        &self,
        domain_hints: Option<&[DomainHint]>,
    ) -> Vec<ToolSummary> {
        let mut all_tools = Vec::new();
        for strategy in &self.strategies {
            let tools = strategy.list_available_tools(domain_hints).await;
            all_tools.extend(tools);
        }
        all_tools
    }
}

#[async_trait]
impl MissingCapabilityStrategy for CompositeMissingCapabilityStrategy {
    fn name(&self) -> &str {
        "composite_missing_capability"
    }

    fn can_handle(&self, _request: &MissingCapabilityRequest) -> bool {
        true
    }

    async fn resolve(
        &self,
        request: &MissingCapabilityRequest,
        context: &ResolutionContext,
    ) -> Result<ResolutionResult, ResolutionError> {
        for strategy in &self.strategies {
            if strategy.can_handle(request) {
                match strategy.resolve(request, context).await {
                    Ok(result) => return Ok(result),
                    Err(ResolutionError::NotFound(_)) => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        Err(ResolutionError::NotFound(format!(
            "No strategy could resolve capability '{}'",
            request.capability_id,
        )))
    }

    async fn list_available_tools(&self, domain_hints: Option<&[DomainHint]>) -> Vec<ToolSummary> {
        self.get_all_available_tools(domain_hints).await
    }
}

/// Pure RTFS Generation Strategy
///
/// Generates capabilities using only RTFS standard library functions
/// without external dependencies.
pub struct PureRtfsGenerationStrategy {
    config: MissingCapabilityStrategyConfig,
    arbiter: Option<Arc<DelegatingArbiter>>,
}

impl PureRtfsGenerationStrategy {
    pub fn new(config: MissingCapabilityStrategyConfig) -> Self {
        Self {
            config,
            arbiter: None,
        }
    }

    pub fn with_arbiter(mut self, arbiter: Arc<DelegatingArbiter>) -> Self {
        self.arbiter = Some(arbiter);
        self
    }

    /// Generate a pure RTFS implementation for a capability
    pub async fn generate_pure_rtfs_implementation(
        &self,
        request: &MissingCapabilityRequest,
    ) -> Result<String, ResolutionError> {
        let capability_id = &request.capability_id;
        let description = request
            .context
            .get("description")
            .cloned()
            .unwrap_or_else(|| format!("Generated capability for {}", capability_id));

        let domain = DomainHint::infer_from_text(capability_id).unwrap_or(DomainHint::Generic);

        let capability_type = Self::infer_capability_type(capability_id).ok_or_else(|| {
            ResolutionError::NotFound(format!(
                "Could not infer capability type for '{}'",
                capability_id
            ))
        })?;

        let rtfs_code = match capability_type {
            IntentType::DataTransform { transform } => self.generate_data_transform_implementation(
                capability_id,
                &description,
                transform,
                &domain,
            ),
            IntentType::ApiCall { action } => {
                self.generate_api_call_implementation(capability_id, &description, action, &domain)
            }
            IntentType::UserInput { prompt_topic } => {
                self.generate_user_input_implementation(capability_id, &description, &prompt_topic)
            }
            IntentType::Output { format } => {
                self.generate_output_implementation(capability_id, &description, format)
            }
            IntentType::Composite => {
                self.generate_composite_implementation(capability_id, &description, &domain)
            }
        }?;

        Ok(rtfs_code)
    }

    /// Infer capability type from capability ID
    fn infer_capability_type(capability_id: &str) -> Option<IntentType> {
        // Split by . or / to handle paths
        let last_segment = capability_id
            .split(|c| c == '.' || c == '/')
            .last()
            .unwrap_or(capability_id);

        if last_segment.starts_with("filter_")
            || last_segment.starts_with("sort_")
            || last_segment.starts_with("transform_")
            || last_segment.starts_with("process_")
        {
            Some(IntentType::DataTransform {
                transform: TransformType::Other(last_segment.to_string()),
            })
        } else if last_segment.starts_with("list_")
            || last_segment.starts_with("get_")
            || last_segment.starts_with("fetch_")
            || last_segment.starts_with("retrieve_")
        {
            Some(IntentType::ApiCall {
                action: ApiAction::List,
            })
        } else if last_segment.starts_with("create_")
            || last_segment.starts_with("add_")
            || last_segment.starts_with("new_")
            || last_segment.starts_with("make_")
        {
            Some(IntentType::ApiCall {
                action: ApiAction::Create,
            })
        } else if last_segment.starts_with("update_")
            || last_segment.starts_with("edit_")
            || last_segment.starts_with("modify_")
            || last_segment.starts_with("change_")
        {
            Some(IntentType::ApiCall {
                action: ApiAction::Update,
            })
        } else if last_segment.starts_with("delete_") || last_segment.starts_with("remove_") {
            Some(IntentType::ApiCall {
                action: ApiAction::Delete,
            })
        } else if last_segment.starts_with("ask_")
            || last_segment.starts_with("prompt_")
            || last_segment.starts_with("input_")
            || last_segment.contains("user_")
        {
            let prompt_topic = last_segment
                .replace("ask_", "")
                .replace("prompt_", "")
                .replace("input_", "")
                .replace("user_", "");
            Some(IntentType::UserInput { prompt_topic })
        } else if last_segment.starts_with("print_")
            || last_segment.starts_with("display_")
            || last_segment.starts_with("output_")
            || last_segment.starts_with("show_")
        {
            Some(IntentType::Output {
                format: OutputFormat::Display,
            })
        } else {
            None
        }
    }

    /// Generate implementation for data transformation capabilities
    fn generate_data_transform_implementation(
        &self,
        capability_id: &str,
        description: &str,
        transform: TransformType,
        _domain: &DomainHint,
    ) -> Result<String, ResolutionError> {
        let transform_name: String = match transform {
            TransformType::Filter => "filter".to_string(),
            TransformType::Sort => "sort".to_string(),
            TransformType::GroupBy => "group-by".to_string(),
            TransformType::Count => "count".to_string(),
            TransformType::Aggregate => "aggregate".to_string(),
            TransformType::Format => "format".to_string(),
            TransformType::Extract => "extract".to_string(),
            TransformType::Parse => "parse".to_string(),
            TransformType::Validate => "validate".to_string(),
            TransformType::Other(name) => name,
        };

        let rtfs_code = format!(
            r#"(capability "{cap_id}"
  :name "{cap_name}"
  :description "{desc}"
  :version "1.0.0"
  :language "rtfs20"
  :permissions [:data_processing]
  :effects [:pure]
  :input-schema [:map [:data [:vector :any]] [:predicate :any]]
  :output-schema :vector
  :implementation
    (fn [input]
      (let [data (get input :data) predicate (get input :predicate)]
        (if (and (vector? data) (fn? predicate))
          ({transform} predicate data)
          []))))
)"#,
            cap_id = capability_id,
            cap_name = capability_id.split('.').last().unwrap_or(capability_id),
            desc = description,
            transform = transform_name
        );

        Ok(rtfs_code)
    }

    /// Generate implementation for API call capabilities
    fn generate_api_call_implementation(
        &self,
        capability_id: &str,
        description: &str,
        action: ApiAction,
        _domain: &DomainHint,
    ) -> Result<String, ResolutionError> {
        // For pure RTFS generation, we create a mock implementation
        // that doesn't actually call external APIs but provides the structure
        let keywords = action.matching_keywords();
        let action_keyword = keywords.first().copied().unwrap_or("api_call");
        let rtfs_code = format!(
            r#"(capability "{}"
  :name "{}"
  :description "{} (Pure RTFS mock implementation)"
  :version "1.0.0"
  :language "rtfs20"
  :permissions [:data_processing]
  :effects [:pure]
  :input-schema :any
  :output-schema :any
  :implementation
    (fn [input]
      ;; Pure RTFS mock implementation for {}
      ;; In a real implementation, this would call an external API
      (do
        (println "Mock {} implementation for {}")
        (cond
          (= :action "list") [{{:id 1 :mock true}} {{:id 2 :mock true}}]
          (= :action "get") (first [{{:id 1 :mock true}}])
          (= :action "create") {{:id 1 :status "created" :mock true}}
          (= :action "update") {{:id (get input :id) :status "updated" :mock true}}
          (= :action "delete") {{:status "deleted" :mock true}}
          :else {{:status "mock" :action "{}" :input input}}))))
)"#,
            capability_id,
            capability_id.split('.').last().unwrap_or(capability_id),
            description,
            action_keyword,
            action_keyword,
            capability_id,
            action_keyword
        );

        Ok(rtfs_code)
    }

    /// Generate implementation for user input capabilities
    fn generate_user_input_implementation(
        &self,
        capability_id: &str,
        description: &str,
        prompt_topic: &str,
    ) -> Result<String, ResolutionError> {
        let prompt = Self::generate_user_prompt(prompt_topic);

        let rtfs_code = format!(
            r#"(capability "{}"
  :name "{}"
  :description "{}"
  :version "1.0.0"
  :language "rtfs20"
  :permissions [:user_interaction]
  :effects [:pure]
  :input-schema (map :default :any)
  :output-schema :string
  :implementation
    (fn [input]
      (let [default_value (get input :default nil)
            prompt "{}"]
        ;; In a real implementation, this would call ccos.user.ask
        ;; For pure RTFS, we return a mock response
        (println "Mock user input for:" prompt)
        (if default_value
          default_value
          "user-provided-value"))))
)"#,
            capability_id,
            capability_id.split('.').last().unwrap_or(capability_id),
            description,
            prompt
        );

        Ok(rtfs_code)
    }

    /// Generate implementation for output capabilities
    fn generate_output_implementation(
        &self,
        capability_id: &str,
        description: &str,
        format: OutputFormat,
    ) -> Result<String, ResolutionError> {
        let format_name: String = match format {
            OutputFormat::Display => "display".to_string(),
            OutputFormat::Print => "print".to_string(),
            OutputFormat::Json => "json".to_string(),
            OutputFormat::Table => "table".to_string(),
            OutputFormat::Summary => "summary".to_string(),
            OutputFormat::Other(name) => name,
        };

        let rtfs_code = format!(
            r#"(capability "{}"
  :name "{}"
  :description "{}"
  :version "1.0.0"
  :language "rtfs20"
  :permissions [:output]
  :effects [:pure]
  :input-schema :any
  :output-schema :nil
  :implementation
    (fn [input]
      ;; Pure RTFS output implementation
      (println "{}" input)
      nil))
)"#,
            capability_id,
            capability_id.split('.').last().unwrap_or(capability_id),
            description,
            format_name
        );

        Ok(rtfs_code)
    }

    /// Generate implementation for composite capabilities
    fn generate_composite_implementation(
        &self,
        capability_id: &str,
        description: &str,
        _domain: &DomainHint,
    ) -> Result<String, ResolutionError> {
        let rtfs_code = format!(
            r#"(capability "{}"
  :name "{}"
  :description "{}"
  :version "1.0.0"
  :language "rtfs20"
  :permissions [:data_processing]
  :effects [:pure]
  :input-schema :any
  :output-schema :any
  :implementation
    (fn [input]
      ;; Composite capability implementation
      ;; This would typically be decomposed into multiple steps
      (do
        (println "Executing composite capability: {}")
        input)))
)"#,
            capability_id,
            capability_id.split('.').last().unwrap_or(capability_id),
            description,
            capability_id
        );

        Ok(rtfs_code)
    }

    /// Generate a user prompt for user input implementations
    fn generate_user_prompt(topic: &str) -> String {
        let filler_words = ["first", "please", "the", "a", "an", "now", "then"];
        let topic_clean: String = topic
            .to_lowercase()
            .replace(['_', '-'], " ")
            .split_whitespace()
            .filter(|w| !filler_words.contains(w))
            .collect::<Vec<_>>()
            .join(" ");

        match topic_clean.as_str() {
            "page size" | "per page" | "limit" => {
                "How many items per page? (e.g., 10, 25, 50)".to_string()
            }
            "page number" | "page" => "Which page number? (starting from 1)".to_string(),
            "sort by" | "sort" => "Sort by which field?".to_string(),
            "direction" | "order" => {
                "Sort direction: asc (ascending) or desc (descending)?".to_string()
            }
            "query" | "search" => "Search query (keywords to find)".to_string(),
            "filter" | "filters" => "Filter criteria".to_string(),
            _ => format!("Please provide: {}", topic_clean),
        }
    }
}

#[async_trait]
impl MissingCapabilityStrategy for PureRtfsGenerationStrategy {
    fn name(&self) -> &str {
        "pure_rtfs_generation"
    }

    fn can_handle(&self, _request: &MissingCapabilityRequest) -> bool {
        self.config.enable_pure_rtfs
    }

    async fn resolve(
        &self,
        request: &MissingCapabilityRequest,
        _context: &ResolutionContext,
    ) -> Result<ResolutionResult, ResolutionError> {
        if request.attempt_count >= self.config.max_attempts {
            return Err(ResolutionError::Internal(format!(
                "Max attempts ({}) exceeded for pure RTFS generation",
                self.config.max_attempts,
            )));
        }

        match self.generate_pure_rtfs_implementation(request).await {
            Ok(_) => Ok(ResolutionResult::Resolved {
                capability_id: request.capability_id.clone(),
                resolution_method: self.name().to_string(),
                provider_info: Some("pure_rtfs_generated".to_string()),
            }),
            Err(e) => Err(ResolutionError::Internal(format!(
                "Pure RTFS generation failed: {}",
                e,
            ))),
        }
    }
}

/// User Interaction Strategy
///
/// Asks the user for clarification or implementation guidance
pub struct UserInteractionStrategy {
    config: MissingCapabilityStrategyConfig,
    marketplace: Option<Arc<CapabilityMarketplace>>,
}

impl UserInteractionStrategy {
    pub fn new(config: MissingCapabilityStrategyConfig) -> Self {
        Self {
            config,
            marketplace: None,
        }
    }

    pub fn with_marketplace(mut self, marketplace: Arc<CapabilityMarketplace>) -> Self {
        self.marketplace = Some(marketplace);
        self
    }
}

#[async_trait]
impl MissingCapabilityStrategy for UserInteractionStrategy {
    fn name(&self) -> &str {
        "user_interaction"
    }

    fn can_handle(&self, _request: &MissingCapabilityRequest) -> bool {
        self.config.enable_user_interaction
    }

    async fn resolve(
        &self,
        request: &MissingCapabilityRequest,
        _context: &ResolutionContext,
    ) -> Result<ResolutionResult, ResolutionError> {
        // Check if we should actually be interactive (via environment variable)
        // Default to non-interactive to not block CI/tests unless requested
        let is_interactive = std::env::var("CCOS_INTERACTIVE")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);

        println!("\n🤖 MISSING CAPABILITY: {}", request.capability_id);
        println!("   I couldn't find this capability automatically.");

        // Suggest alternatives from marketplace
        if let Some(marketplace) = &self.marketplace {
            // Extract the last part of the capability ID (e.g., "list_issues") to search for
            let search_term = request
                .capability_id
                .split('.')
                .last()
                .unwrap_or(&request.capability_id);
            // Search for capabilities containing this term
            let candidates = marketplace.search_by_id(search_term).await;

            if !candidates.is_empty() {
                println!("   Did you mean one of these?");
                for (i, cap) in candidates.iter().take(3).enumerate() {
                    println!("   {}. {} ({})", i + 1, cap.id, cap.description);
                }
            }
        }

        println!("   How would you like to resolve this?");
        println!("   1. Generate Pure RTFS implementation (mock)");
        println!("   2. Search for generic alternative");
        println!("   3. Skip resolution (fail)");

        let choice = if is_interactive {
            use std::io::{self, Write};
            print!("   > ");
            io::stdout().flush().unwrap_or(());

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                input.trim().to_string()
            } else {
                "3".to_string() // Default to fail on error
            }
        } else {
            println!("   > (Simulated User Input: 1)");
            "1".to_string()
        };

        match choice.as_str() {
            "1" => {
                // Delegate to Pure RTFS generation strategy
                let strategy = PureRtfsGenerationStrategy::new(self.config.clone());
                match strategy.generate_pure_rtfs_implementation(request).await {
                    Ok(rtfs_source) => {
                        Ok(ResolutionResult::Resolved {
                            capability_id: request.capability_id.clone(), // We resolve for the requested ID
                            resolution_method: "user_selected_pure_rtfs".to_string(),
                            provider_info: Some(rtfs_source), // We pass the source back
                        })
                    }
                    Err(e) => Err(e),
                }
            }
            "2" => Err(ResolutionError::NotFound(
                "Generic search not implemented".to_string(),
            )),
            _ => Err(ResolutionError::NotFound(
                "User skipped resolution".to_string(),
            )),
        }
    }
}

/// External LLM Hint Strategy
///
/// Queries an external LLM for implementation suggestions
pub struct ExternalLlmHintStrategy {
    config: MissingCapabilityStrategyConfig,
    arbiter: Option<Arc<DelegatingArbiter>>,
}

impl ExternalLlmHintStrategy {
    pub fn new(config: MissingCapabilityStrategyConfig) -> Self {
        Self {
            config,
            arbiter: None,
        }
    }

    pub fn with_arbiter(mut self, arbiter: Arc<DelegatingArbiter>) -> Self {
        self.arbiter = Some(arbiter);
        self
    }
}

#[async_trait]
impl MissingCapabilityStrategy for ExternalLlmHintStrategy {
    fn name(&self) -> &str {
        "llm_synthesis"
    }

    fn can_handle(&self, _request: &MissingCapabilityRequest) -> bool {
        self.config.enable_external_llm && self.arbiter.is_some()
    }

    async fn resolve(
        &self,
        request: &MissingCapabilityRequest,
        _context: &ResolutionContext,
    ) -> Result<ResolutionResult, ResolutionError> {
        match self.generate_implementation(request).await {
            Ok(_) => {
                // For the trait implementation, we assume the caller will handle registration
                // if they use the specific method, but here we just claim success.
                Ok(ResolutionResult::Resolved {
                    capability_id: request.capability_id.clone(),
                    resolution_method: self.name().to_string(),
                    provider_info: Some("llm_generated".to_string()),
                })
            }
            Err(e) => Err(e),
        }
    }
}

impl ExternalLlmHintStrategy {
    /// Generate RTFS implementation using external LLM
    pub async fn generate_implementation(
        &self,
        request: &MissingCapabilityRequest,
    ) -> Result<String, ResolutionError> {
        let arbiter = self.arbiter.as_ref().ok_or_else(|| {
            ResolutionError::Internal("Arbiter not configured for LLM synthesis".to_string())
        })?;

        let prompt = format!(
            "How would you implement a capability called '{}' in RTFS?\n\
             Provide ONLY the RTFS code block starting with (capability ...).\n\
             Context: {:?}",
            request.capability_id, request.context
        );

        match arbiter.query_llm(&prompt).await {
            Ok(response) => {
                // Extract code block if present
                let code = if let Some(start) = response.find("```") {
                    let rest = &response[start..];
                    if let Some(end) = rest[3..].find("```") {
                        // Skip language tag if present
                        let content = &rest[3..end + 3];
                        let newline = content.find('\n').unwrap_or(0);
                        content[newline..].trim().to_string()
                    } else {
                        response.trim().to_string()
                    }
                } else {
                    response.trim().to_string()
                };

                // Validate it looks like RTFS
                if code.starts_with("(capability") {
                    Ok(code.to_string())
                } else {
                    Err(ResolutionError::Internal(
                        "LLM did not return valid RTFS code".to_string(),
                    ))
                }
            }
            Err(e) => Err(ResolutionError::Internal(format!(
                "LLM query failed: {}",
                e
            ))),
        }
    }
}

/// Service Discovery Hint Strategy
///
/// Asks for hints about where to find a capability
pub struct ServiceDiscoveryHintStrategy {
    config: MissingCapabilityStrategyConfig,
}

impl ServiceDiscoveryHintStrategy {
    pub fn new(config: MissingCapabilityStrategyConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl MissingCapabilityStrategy for ServiceDiscoveryHintStrategy {
    fn name(&self) -> &str {
        "service_discovery_hint"
    }

    fn can_handle(&self, _request: &MissingCapabilityRequest) -> bool {
        self.config.enable_service_discovery
    }

    async fn resolve(
        &self,
        request: &MissingCapabilityRequest,
        _context: &ResolutionContext,
    ) -> Result<ResolutionResult, ResolutionError> {
        // Placeholder for service discovery hint logic
        // This would typically query a known registry or ask the user for a URL/name

        Err(ResolutionError::NotFound(format!(
            "No service discovery hints available for {}",
            request.capability_id
        )))
    }
}
