//! Server Trust and User Interaction System
//!
//! Generic server trust management with user interaction for capability resolution.
//! Provides configurable trust policies and interactive server selection.

use crate::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use url::Url;

/// Trust level for MCP servers
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Unknown third-party servers (lowest priority)
    Unverified,
    /// Community-verified servers
    Verified,
    /// User-approved servers
    Approved,
    /// Official/verified servers (highest priority)
    Official,
}

/// Server trust information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTrustInfo {
    pub domain: String,
    pub provider: String,
    pub trust_level: TrustLevel,
    pub verified: bool,
    pub notes: String,
    pub last_verified: Option<String>,
}

/// Trust policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustPolicy {
    /// Whether to require user approval for unknown servers
    pub require_approval_for_unknown: bool,
    /// Whether to auto-select official servers
    pub auto_select_official: bool,
    /// Whether to auto-select approved servers
    pub auto_select_approved: bool,
    /// Minimum trust level to auto-select
    pub min_auto_select_trust: TrustLevel,
    /// Whether to prompt for server selection when multiple options exist
    pub prompt_for_selection: bool,
    /// Maximum number of servers to show in selection prompt
    pub max_selection_display: usize,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            require_approval_for_unknown: true,
            auto_select_official: true,
            auto_select_approved: true,
            min_auto_select_trust: TrustLevel::Verified,
            prompt_for_selection: true,
            max_selection_display: 10,
        }
    }
}

/// Server trust registry
#[derive(Debug, Clone)]
pub struct ServerTrustRegistry {
    /// Trusted servers by domain
    trusted_servers: HashMap<String, ServerTrustInfo>,
    /// User-approved servers
    user_approved: HashSet<String>,
    /// Trust policy
    policy: TrustPolicy,
}

impl ServerTrustRegistry {
    /// Create a new server trust registry with default policy
    pub fn new() -> Self {
        Self {
            trusted_servers: HashMap::new(),
            user_approved: HashSet::new(),
            policy: TrustPolicy::default(),
        }
    }

    /// Create a new server trust registry with custom policy
    pub fn with_policy(policy: TrustPolicy) -> Self {
        Self {
            trusted_servers: HashMap::new(),
            user_approved: HashSet::new(),
            policy,
        }
    }

    /// Add an official server to the registry
    pub fn add_official(&mut self, domain: &str, provider: &str, notes: &str) {
        self.trusted_servers.insert(
            domain.to_string(),
            ServerTrustInfo {
                domain: domain.to_string(),
                provider: provider.to_string(),
                trust_level: TrustLevel::Official,
                verified: true,
                notes: notes.to_string(),
                last_verified: Some(chrono::Utc::now().to_rfc3339()),
            },
        );
    }

    /// Add a verified server to the registry
    pub fn add_verified(&mut self, domain: &str, provider: &str, notes: &str) {
        self.trusted_servers.insert(
            domain.to_string(),
            ServerTrustInfo {
                domain: domain.to_string(),
                provider: provider.to_string(),
                trust_level: TrustLevel::Verified,
                verified: true,
                notes: notes.to_string(),
                last_verified: Some(chrono::Utc::now().to_rfc3339()),
            },
        );
    }

    /// Approve a server (user action)
    pub fn approve_server(&mut self, domain: &str) {
        self.user_approved.insert(domain.to_string());

        // Update trust info if server exists
        if let Some(trust_info) = self.trusted_servers.get_mut(domain) {
            trust_info.trust_level = TrustLevel::Approved;
            trust_info.last_verified = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    /// Get trust level for a server domain
    pub fn get_trust_level(&self, domain: &str) -> TrustLevel {
        // Check if user-approved
        if self.user_approved.contains(domain) {
            return TrustLevel::Approved;
        }

        // Check if in trusted registry
        if let Some(trust_info) = self.trusted_servers.get(domain) {
            return trust_info.trust_level.clone();
        }

        // Default to unverified
        TrustLevel::Unverified
    }

    /// Check if a server is trusted
    pub fn is_trusted(&self, domain: &str) -> bool {
        let trust_level = self.get_trust_level(domain);
        matches!(
            trust_level,
            TrustLevel::Official | TrustLevel::Approved | TrustLevel::Verified
        )
    }

    /// Check if a server should be auto-selected
    pub fn should_auto_select(&self, domain: &str) -> bool {
        let trust_level = self.get_trust_level(domain);

        match trust_level {
            TrustLevel::Official => self.policy.auto_select_official,
            TrustLevel::Approved => self.policy.auto_select_approved,
            TrustLevel::Verified => self.policy.min_auto_select_trust <= TrustLevel::Verified,
            TrustLevel::Unverified => false,
        }
    }

    /// Check if user approval is required for a server
    pub fn requires_approval(&self, domain: &str) -> bool {
        let trust_level = self.get_trust_level(domain);

        match trust_level {
            TrustLevel::Unverified => self.policy.require_approval_for_unknown,
            _ => false,
        }
    }

    /// Get server trust information
    pub fn get_trust_info(&self, domain: &str) -> Option<&ServerTrustInfo> {
        self.trusted_servers.get(domain)
    }

    /// Update trust policy
    pub fn set_policy(&mut self, policy: TrustPolicy) {
        self.policy = policy;
    }

    /// Get current policy
    pub fn policy(&self) -> &TrustPolicy {
        &self.policy
    }
}

/// Server selection result
#[derive(Debug, Clone)]
pub struct ServerSelectionResult {
    pub selected_domain: String,
    pub selection_method: SelectionMethod,
    pub user_approved: bool,
}

#[derive(Debug, Clone)]
pub enum SelectionMethod {
    AutoSelected,
    UserSelected,
    UserApproved,
}

/// Interactive server selection handler
pub struct ServerSelectionHandler {
    trust_registry: ServerTrustRegistry,
}

impl ServerSelectionHandler {
    /// Create a new server selection handler
    pub fn new(trust_registry: ServerTrustRegistry) -> Self {
        Self { trust_registry }
    }

    /// Select the best server from a list of candidates
    pub async fn select_server(
        &mut self,
        capability_id: &str,
        candidates: Vec<ServerCandidate>,
    ) -> RuntimeResult<ServerSelectionResult> {
        if candidates.is_empty() {
            return Err(RuntimeError::Generic(
                "No server candidates provided".to_string(),
            ));
        }

        // Filter candidates by trust level
        let trusted_candidates: Vec<_> = candidates
            .iter()
            .filter(|candidate| self.trust_registry.is_trusted(&candidate.domain))
            .cloned()
            .collect();

        // If we have trusted candidates, prefer them
        let candidates_to_consider = if !trusted_candidates.is_empty() {
            trusted_candidates
        } else {
            candidates
        };

        // Check if we can auto-select
        for candidate in &candidates_to_consider {
            if self.trust_registry.should_auto_select(&candidate.domain) {
                eprintln!("✅ AUTO-SELECT: Using trusted server: {}", candidate.domain);
                return Ok(ServerSelectionResult {
                    selected_domain: candidate.domain.clone(),
                    selection_method: SelectionMethod::AutoSelected,
                    user_approved: false,
                });
            }
        }

        // Check if user approval is required
        let requires_approval = candidates_to_consider
            .iter()
            .any(|candidate| self.trust_registry.requires_approval(&candidate.domain));

        if requires_approval {
            return self
                .handle_user_approval(capability_id, candidates_to_consider)
                .await;
        }

        // If policy allows, prompt for selection
        if self.trust_registry.policy().prompt_for_selection && candidates_to_consider.len() > 1 {
            return self
                .handle_interactive_selection(capability_id, candidates_to_consider)
                .await;
        }

        // Default: select the first candidate
        let selected = &candidates_to_consider[0];
        eprintln!(
            "🤖 DEFAULT: Auto-selecting first server: {}",
            selected.domain
        );

        Ok(ServerSelectionResult {
            selected_domain: selected.domain.clone(),
            selection_method: SelectionMethod::AutoSelected,
            user_approved: false,
        })
    }

    /// Handle user approval for unknown servers
    async fn handle_user_approval(
        &mut self,
        capability_id: &str,
        mut candidates: Vec<ServerCandidate>,
    ) -> RuntimeResult<ServerSelectionResult> {
        use std::io::{self, Write};

        loop {
            // Limit display to top N servers, but keep all for selection
            let max_display = self.trust_registry.policy().max_selection_display.min(10);
            let display_count = candidates.len().min(max_display);

            eprintln!(
                "\n⚠️  UNKNOWN SERVERS: Found {} server(s) for '{}'",
                candidates.len(),
                capability_id
            );
            eprintln!("These servers are not in the trusted registry.");
            if candidates.len() == 1 {
                eprintln!(
                    "(Only 1 server scored >= 0.3 relevance threshold from initial discovery)\n"
                );
            } else {
                eprintln!("\nShowing top {} ranked by relevance:\n", display_count);
            }

            for (i, candidate) in candidates.iter().take(display_count).enumerate() {
                eprintln!(
                    "  {}. {} - {}",
                    i + 1,
                    candidate.domain,
                    candidate.description
                );
                if let Some(ref repository) = candidate.repository {
                    eprintln!("     Repository: {}", repository);
                }
                eprintln!("     Score: {:.2}", candidate.score);
                eprintln!();
            }

            if candidates.len() > display_count {
                eprintln!(
                    "... and {} more server(s) not shown\n",
                    candidates.len() - display_count
                );
            }

            eprintln!("Enter a number (1-{}) to select a server", display_count);
            eprintln!(
                "Enter 'a' to approve all {} servers for future use",
                candidates.len()
            );
            if candidates.len() > display_count {
                eprintln!("Enter 'm' to see more servers");
            }
            if candidates.len() > 1 {
                eprintln!("Enter 'r' to refine your search with a hint");
            }
            eprintln!("Enter 'd' to deny and cancel resolution");
            eprintln!("Enter 'u' to add a server URL and save to overrides");
            if candidates.len() == 1 {
                eprintln!("\n💡 Tip: If this server doesn't match your needs, try a different search query");
            }
            eprint!("\nYour choice: ");

            // Read user input from stdin
            io::stdout()
                .flush()
                .map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;

            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .map_err(|e| RuntimeError::Generic(format!("Failed to read user input: {}", e)))?;

            let choice = input.trim().to_lowercase();

            match choice.as_str() {
                "d" => {
                    eprintln!("❌ User denied server approval. Resolution cancelled.");
                    return Err(RuntimeError::Generic(
                        "User denied server approval".to_string(),
                    ));
                }
                "a" => {
                    eprintln!(
                        "✅ User approved all {} servers for future use.",
                        candidates.len()
                    );
                    // Approve all servers
                    for candidate in &candidates {
                        self.trust_registry.approve_server(&candidate.domain);
                    }
                    // Select the first one
                    let selected = &candidates[0];
                    eprintln!("✅ Selected server: {}", selected.domain);

                    return Ok(ServerSelectionResult {
                        selected_domain: selected.domain.clone(),
                        selection_method: SelectionMethod::UserApproved,
                        user_approved: true,
                    });
                }
                "m" => {
                    // Show more servers
                    eprintln!("\n📋 All {} servers:\n", candidates.len());
                    for (i, candidate) in candidates.iter().enumerate() {
                        eprintln!(
                            "  {}. {} - {}",
                            i + 1,
                            candidate.domain,
                            candidate.description
                        );
                        if let Some(ref repository) = candidate.repository {
                            eprintln!("     Repository: {}", repository);
                        }
                        eprintln!("     Score: {:.2}", candidate.score);
                        eprintln!();
                    }

                    // Recursively call this function to re-prompt with full list
                    // (This will re-display but with full list awareness)
                    eprintln!("Enter a number (1-{}) to select a server", candidates.len());
                    eprintln!("Enter 'a' to approve all servers");
                    eprintln!("Enter 'r' to refine your search");
                    eprintln!("Enter 'd' to deny and cancel");
                    eprint!("\nYour choice: ");
                    io::stdout().flush().map_err(|e| {
                        RuntimeError::Generic(format!("Failed to flush stdout: {}", e))
                    })?;

                    // Read next input
                    let mut input2 = String::new();
                    io::stdin().read_line(&mut input2).map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read user input: {}", e))
                    })?;
                    let choice2 = input2.trim();

                    // Parse the second choice
                    return self.parse_selection_choice(choice2, &candidates).await;
                }
                "r" => {
                    // Refine search with a hint
                    eprintln!("\n🔍 Enter a search hint to filter servers (e.g., 'github', 'obsidian', 'official'):");
                    eprint!("Hint: ");
                    io::stdout().flush().map_err(|e| {
                        RuntimeError::Generic(format!("Failed to flush stdout: {}", e))
                    })?;

                    let mut hint_input = String::new();
                    io::stdin().read_line(&mut hint_input).map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read user input: {}", e))
                    })?;
                    let hint = hint_input.trim().to_lowercase();

                    if hint.is_empty() {
                        eprintln!("❌ Empty hint. Showing all servers again.");
                        continue; // Loop again with same candidates
                    }

                    // Filter candidates by hint
                    let filtered: Vec<_> = candidates
                        .iter()
                        .filter(|c| {
                            let name_lower = c.name.to_lowercase();
                            let desc_lower = c.description.to_lowercase();
                            let domain_lower = c.domain.to_lowercase();
                            name_lower.contains(&hint)
                                || desc_lower.contains(&hint)
                                || domain_lower.contains(&hint)
                        })
                        .cloned()
                        .collect();

                    if filtered.is_empty() {
                        eprintln!(
                            "❌ No servers match hint '{}'. Showing all servers again.",
                            hint
                        );
                        continue; // Loop again with same candidates
                    }

                    eprintln!("✅ Found {} server(s) matching '{}'", filtered.len(), hint);
                    // Update candidates and loop again
                    candidates = filtered;
                    continue;
                }
                "u" => {
                    // Allow user to add a MCP server URL and persist it to overrides
                    eprintln!("\n➕ Add MCP server by URL");
                    eprintln!("Enter MCP server URL (e.g., wss://mcp.example.com or https://github.com/org/repo):");
                    eprint!("URL: ");
                    io::stdout().flush().map_err(|e| {
                        RuntimeError::Generic(format!("Failed to flush stdout: {}", e))
                    })?;

                    let mut url_input = String::new();
                    io::stdin().read_line(&mut url_input).map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read user input: {}", e))
                    })?;
                    let url_input = url_input.trim();

                    if url_input.is_empty() {
                        eprintln!("❌ Empty URL. Returning to selection.");
                        continue;
                    }

                    // Derive a default name from the URL
                    let (default_name, repo_source) = Self::derive_default_server_name(url_input);
                    eprintln!(
                        "Optional: enter a short server name (default: {})",
                        default_name
                    );
                    eprint!("Name: ");
                    io::stdout().flush().map_err(|e| {
                        RuntimeError::Generic(format!("Failed to flush stdout: {}", e))
                    })?;
                    let mut name_input = String::new();
                    io::stdin().read_line(&mut name_input).map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read user input: {}", e))
                    })?;
                    let name = name_input.trim();
                    let server_name = if name.is_empty() {
                        default_name.clone()
                    } else {
                        name.to_string()
                    };

                    let description = format!("User-added MCP server for {}", capability_id);

                    let server = Self::build_mcp_server_from_url(
                        &server_name,
                        &description,
                        url_input,
                        repo_source,
                    );

                    // Persist to overrides.json
                    if let Err(e) = Self::save_override_for(capability_id, &server) {
                        eprintln!("❌ Failed to save to overrides.json: {}", e);
                        continue;
                    }

                    eprintln!("✅ Saved to overrides.json");

                    // Add to current candidates and prefer it by setting a high score
                    let domain = Self::derive_domain_for_candidate(url_input, &server_name);
                    let repo = server.repository.as_ref().map(|r| r.url.clone());
                    let new_candidate =
                        ServerCandidate::new(domain, server_name.clone(), description)
                            .with_repository(repo.unwrap_or_default())
                            .with_score(0.95);

                    candidates.insert(0, new_candidate);
                    // Loop again to show updated list
                    continue;
                }
                _ => {
                    // Try to parse as a number
                    return self.parse_selection_choice(&choice, &candidates).await;
                }
            }
        } // end loop
    }

    /// Handle interactive server selection
    async fn handle_interactive_selection(
        &mut self,
        capability_id: &str,
        candidates: Vec<ServerCandidate>,
    ) -> RuntimeResult<ServerSelectionResult> {
        eprintln!(
            "\n🔍 MULTIPLE SERVERS: Found {} server(s) for '{}'",
            candidates.len(),
            capability_id
        );
        eprintln!("Please select the most appropriate one:\n");

        let display_count = candidates
            .len()
            .min(self.trust_registry.policy().max_selection_display);

        for (i, candidate) in candidates.iter().take(display_count).enumerate() {
            let trust_level = self.trust_registry.get_trust_level(&candidate.domain);
            let trust_indicator = match trust_level {
                TrustLevel::Official => "✅ OFFICIAL",
                TrustLevel::Approved => "✅ APPROVED",
                TrustLevel::Verified => "✅ VERIFIED",
                TrustLevel::Unverified => "⚠️  UNKNOWN",
            };

            eprintln!("{}. {} {}", i + 1, candidate.domain, trust_indicator);
            eprintln!("   Description: {}", candidate.description);
            if let Some(ref repository) = candidate.repository {
                eprintln!("   Repository: {}", repository);
            }
            eprintln!();
        }

        eprint!("Your choice [1-{}]: ", display_count);

        // Read user input from stdin
        use std::io::{self, Write};
        io::stdout()
            .flush()
            .map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read user input: {}", e)))?;

        let choice = input.trim();

        // Try to parse as a number
        if let Ok(index) = choice.parse::<usize>() {
            if index >= 1 && index <= display_count {
                let selected = &candidates[index - 1];
                eprintln!("✅ User selected server: {}", selected.domain);

                Ok(ServerSelectionResult {
                    selected_domain: selected.domain.clone(),
                    selection_method: SelectionMethod::UserSelected,
                    user_approved: false,
                })
            } else {
                eprintln!(
                    "❌ Invalid selection: {}. Must be between 1 and {}",
                    index, display_count
                );
                Err(RuntimeError::Generic(format!(
                    "Invalid selection: {}",
                    index
                )))
            }
        } else {
            eprintln!(
                "❌ Invalid input: '{}'. Expected a number between 1 and {}",
                choice, display_count
            );
            Err(RuntimeError::Generic(format!("Invalid input: {}", choice)))
        }
    }

    /// Parse numeric selection choice
    async fn parse_selection_choice(
        &mut self,
        choice: &str,
        candidates: &[ServerCandidate],
    ) -> RuntimeResult<ServerSelectionResult> {
        // Try to parse as a number
        if let Ok(index) = choice.parse::<usize>() {
            if index >= 1 && index <= candidates.len() {
                let selected = &candidates[index - 1];
                eprintln!("✅ User selected server: {}", selected.domain);

                // Approve the selected server
                self.trust_registry.approve_server(&selected.domain);

                Ok(ServerSelectionResult {
                    selected_domain: selected.domain.clone(),
                    selection_method: SelectionMethod::UserApproved,
                    user_approved: true,
                })
            } else {
                eprintln!(
                    "❌ Invalid selection: {}. Must be between 1 and {}",
                    index,
                    candidates.len()
                );
                Err(RuntimeError::Generic(format!(
                    "Invalid selection: {}",
                    index
                )))
            }
        } else {
            eprintln!(
                "❌ Invalid input: '{}'. Expected a number (1-{}), 'a', 'm', 'r', or 'd'",
                choice,
                candidates.len()
            );
            Err(RuntimeError::Generic(format!("Invalid input: {}", choice)))
        }
    }
}

impl ServerSelectionHandler {
    // Build a minimal McpServer from a URL input
    fn build_mcp_server_from_url(
        name: &str,
        description: &str,
        url_input: &str,
        repo_source: Option<String>,
    ) -> crate::ccos::synthesis::mcp_registry_client::McpServer {
        use crate::ccos::synthesis::mcp_registry_client as mcp;

        // Remote type based on scheme
        let remote_type = if url_input.starts_with("ws://") || url_input.starts_with("wss://") {
            "websocket".to_string()
        } else {
            // Fallback to http descriptor; clients may still connect appropriately
            "http".to_string()
        };

        let repository = match repo_source {
            Some(source) => Some(mcp::McpRepository {
                url: url_input.to_string(),
                source,
            }),
            None => None,
        };

        let remote = mcp::McpRemote {
            r#type: remote_type,
            url: url_input.to_string(),
            headers: None,
        };

        mcp::McpServer {
            schema: None,
            name: name.to_string(),
            description: description.to_string(),
            version: "1.0.0".to_string(),
            repository,
            packages: None,
            remotes: Some(vec![remote]),
        }
    }

    // Derive a default short name from the URL
    fn derive_default_server_name(url_input: &str) -> (String, Option<String>) {
        if let Ok(parsed) = Url::parse(url_input) {
            let host = parsed.host_str().unwrap_or("custom");
            // If it's a GitHub repo URL, try org/repo
            if host == "github.com" {
                let mut segs = parsed.path().trim_matches('/').split('/');
                if let (Some(org), Some(repo)) = (segs.next(), segs.next()) {
                    return (
                        format!("github/{}-{}", org, repo),
                        Some("github".to_string()),
                    );
                }
                return ("github/custom-mcp".to_string(), Some("github".to_string()));
            }
            // Otherwise use host as name
            return (host.to_string(), None);
        }
        ("custom-mcp-server".to_string(), None)
    }

    // Derive a candidate domain from the URL or name
    fn derive_domain_for_candidate(url_input: &str, fallback_name: &str) -> String {
        // Try to align with resolver's extract_domain_from_server_name behavior:
        // Prefer the token before first '/' if present; otherwise token before first '.'; else full name.
        // If URL is a GitHub repo, construct a name like "github/<org>-<repo>" and then apply same rules.
        if let Ok(parsed) = Url::parse(url_input) {
            let mut candidate_name = fallback_name.to_string();
            if let Some(host) = parsed.host_str() {
                if host == "github.com" {
                    let path = parsed.path().trim_matches('/');
                    let mut segs = path.split('/');
                    if let (Some(org), Some(repo)) = (segs.next(), segs.next()) {
                        candidate_name = format!("github/{}-{}", org, repo);
                    } else {
                        candidate_name = "github/custom-mcp".to_string();
                    }
                } else {
                    // For non-GitHub hosts, use host as candidate name
                    candidate_name = host.to_string();
                }
            }
            if let Some(pos) = candidate_name.find('/') {
                return candidate_name[..pos].to_string();
            }
            if let Some(pos) = candidate_name.find('.') {
                return candidate_name[..pos].to_string();
            }
            return candidate_name;
        }
        // No parsable URL; follow same splitting rules on fallback name
        if let Some(pos) = fallback_name.find('/') {
            return fallback_name[..pos].to_string();
        }
        if let Some(pos) = fallback_name.find('.') {
            return fallback_name[..pos].to_string();
        }
        fallback_name.to_string()
    }

    // Save or append an override entry for the capability
    fn save_override_for(
        capability_id: &str,
        server: &crate::ccos::synthesis::mcp_registry_client::McpServer,
    ) -> RuntimeResult<()> {
        // Compute overrides path similar to the resolver
        let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let overrides_path = if current.ends_with("rtfs_compiler") {
            current
                .parent()
                .unwrap_or(&current)
                .join("capabilities/mcp/overrides.json")
        } else {
            current.join("capabilities/mcp/overrides.json")
        };

        // Ensure directory exists
        if let Some(parent) = overrides_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to create overrides directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        #[derive(serde::Serialize, serde::Deserialize)]
        struct OverrideEntry {
            matches: Vec<String>,
            server: crate::ccos::synthesis::mcp_registry_client::McpServer,
        }

        #[derive(serde::Serialize, serde::Deserialize, Default)]
        struct OverrideFile {
            entries: Vec<OverrideEntry>,
        }

        // Read or initialize overrides file
        let mut file: OverrideFile = if overrides_path.exists() {
            let content = fs::read_to_string(&overrides_path).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to read overrides file '{}': {}",
                    overrides_path.display(),
                    e
                ))
            })?;
            serde_json::from_str(&content).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to parse overrides file '{}': {}",
                    overrides_path.display(),
                    e
                ))
            })?
        } else {
            OverrideFile {
                entries: Vec::new(),
            }
        };

        // Build match patterns: exact capability + domain.* if possible
        let mut matches = vec![capability_id.to_string()];
        if let Some(dot) = capability_id.find('.') {
            let domain = &capability_id[..dot];
            matches.push(format!("{}.*", domain));
        }

        // Append entry
        file.entries.push(OverrideEntry {
            matches,
            server: server.clone(),
        });

        // Write back to disk
        let json = serde_json::to_string_pretty(&file).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize overrides file: {}", e))
        })?;
        fs::write(&overrides_path, json).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write overrides file '{}': {}",
                overrides_path.display(),
                e
            ))
        })?;

        Ok(())
    }
}

/// Server candidate for selection
#[derive(Debug, Clone)]
pub struct ServerCandidate {
    pub domain: String,
    pub name: String,
    pub description: String,
    pub repository: Option<String>,
    pub score: f64,
}

impl ServerCandidate {
    /// Create a new server candidate
    pub fn new(domain: String, name: String, description: String) -> Self {
        Self {
            domain,
            name,
            description,
            repository: None,
            score: 0.0,
        }
    }

    /// Set repository information
    pub fn with_repository(mut self, repository: String) -> Self {
        self.repository = Some(repository);
        self
    }

    /// Set relevance score
    pub fn with_score(mut self, score: f64) -> Self {
        self.score = score;
        self
    }
}

/// Default server trust registry with common official servers
pub fn create_default_trust_registry() -> ServerTrustRegistry {
    let mut registry = ServerTrustRegistry::new();

    // Add common official servers (generic patterns)
    registry.add_official("api.github.com", "github", "Official GitHub REST API");
    registry.add_official(
        "api.githubcopilot.com",
        "github",
        "Official GitHub Copilot MCP API",
    );
    registry.add_official(
        "api.openweathermap.org",
        "openweather",
        "Official OpenWeather API",
    );
    registry.add_official("api.stripe.com", "stripe", "Official Stripe API");
    registry.add_official("api.slack.com", "slack", "Official Slack API");
    registry.add_official("api.anthropic.com", "anthropic", "Official Anthropic API");
    registry.add_official("api.openai.com", "openai", "Official OpenAI API");

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trust_registry() {
        let mut registry = ServerTrustRegistry::new();

        // Add official server
        registry.add_official("api.github.com", "github", "Official GitHub API");

        // Test trust levels
        assert_eq!(
            registry.get_trust_level("api.github.com"),
            TrustLevel::Official
        );
        assert_eq!(
            registry.get_trust_level("unknown.com"),
            TrustLevel::Unverified
        );
        assert!(registry.is_trusted("api.github.com"));
        assert!(!registry.is_trusted("unknown.com"));
    }

    #[test]
    fn test_user_approval() {
        let mut registry = ServerTrustRegistry::new();

        // Approve unknown server
        registry.approve_server("custom-api.com");

        assert_eq!(
            registry.get_trust_level("custom-api.com"),
            TrustLevel::Approved
        );
        assert!(registry.is_trusted("custom-api.com"));
    }

    #[test]
    fn test_auto_selection() {
        let mut registry = ServerTrustRegistry::new();
        registry.add_official("api.github.com", "github", "Official GitHub API");

        // Should auto-select official servers by default
        assert!(registry.should_auto_select("api.github.com"));

        // Should not auto-select unknown servers
        assert!(!registry.should_auto_select("unknown.com"));
    }
}
