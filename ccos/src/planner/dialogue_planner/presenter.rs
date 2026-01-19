// DialoguePresenter: Handles formatting and progressive disclosure of discovery results
//
// This module provides clean presentation of discovery results with:
// - Description truncation to prevent overwhelming output
// - Progressive disclosure (summary → top results → show more)
// - Relevance scoring with visual indicators
// - Source icons for different discovery sources
// - Color support (when terminal supports it)

use crate::approval::queue::DiscoverySource;
use crate::discovery::RegistrySearchResult;

/// Configuration for result presentation
#[derive(Debug, Clone)]
pub struct PresenterConfig {
    /// Maximum results to show inline before "show more"
    pub max_inline_results: usize,
    /// Maximum description length before truncation
    pub max_description_length: usize,
    /// Whether to use colors (auto-detected if None)
    pub use_color: Option<bool>,
    /// Whether to show relevance stars
    pub show_relevance: bool,
}

impl Default for PresenterConfig {
    fn default() -> Self {
        Self {
            max_inline_results: 5,
            max_description_length: 80,
            use_color: None, // Auto-detect
            show_relevance: true,
        }
    }
}

/// Handles presentation of dialogue results
pub struct DialoguePresenter {
    config: PresenterConfig,
}

impl DialoguePresenter {
    pub fn new() -> Self {
        Self {
            config: PresenterConfig::default(),
        }
    }

    pub fn with_config(config: PresenterConfig) -> Self {
        Self { config }
    }

    // -------------------------------------------------------------------------
    // Discovery Result Formatting
    // -------------------------------------------------------------------------

    /// Format discovery results with progressive disclosure
    pub fn format_discovery_results(
        &self,
        query: &str,
        results: &[RegistrySearchResult],
        show_all: bool,
    ) -> String {
        if results.is_empty() {
            return self.format_no_results(query);
        }

        let mut output = Vec::new();

        // 1. Summary line
        output.push(self.format_summary(query, results));

        // 2. Top results (or all if show_all)
        let display_count = if show_all {
            results.len()
        } else {
            self.config.max_inline_results.min(results.len())
        };

        output.push(String::new()); // blank line

        for (i, result) in results.iter().take(display_count).enumerate() {
            output.push(self.format_result_line(i + 1, result));
        }

        // 3. "Show more" hint if there are hidden results
        let hidden_count = results.len().saturating_sub(display_count);
        if hidden_count > 0 && !show_all {
            output.push(String::new());
            output.push(format!(
                "📋 {} more result(s) available. Type 'more' to see all.",
                hidden_count
            ));
        }

        // 4. Action hints
        output.push(String::new());
        output.push(
            "💡 Commands: 'details <N>' | 'explore <N>' | 'connect <N>' | 'more' | 'proceed'"
                .to_string(),
        );

        output.join("\n")
    }

    /// Format the summary line with counts by source
    fn format_summary(&self, query: &str, results: &[RegistrySearchResult]) -> String {
        let mcp_count = results
            .iter()
            .filter(|r| matches!(r.source, DiscoverySource::McpRegistry { .. }))
            .count();
        let apis_guru_count = results
            .iter()
            .filter(|r| matches!(r.source, DiscoverySource::ApisGuru { .. }))
            .count();
        let web_count = results
            .iter()
            .filter(|r| matches!(r.source, DiscoverySource::WebSearch { .. }))
            .count();
        let llm_count = results
            .iter()
            .filter(|r| matches!(r.source, DiscoverySource::LlmSuggestion { .. }))
            .count();
        let local_count = results
            .iter()
            .filter(|r| matches!(r.source, DiscoverySource::LocalOverride { .. }))
            .count();

        let mut sources = Vec::new();
        if mcp_count > 0 {
            sources.push(format!("🔧 {} MCP", mcp_count));
        }
        if apis_guru_count > 0 {
            sources.push(format!("🌐 {} API", apis_guru_count));
        }
        if web_count > 0 {
            sources.push(format!("🔍 {} Web", web_count));
        }
        if llm_count > 0 {
            sources.push(format!("🤖 {} LLM", llm_count));
        }
        if local_count > 0 {
            sources.push(format!("📂 {} Local", local_count));
        }

        format!(
            "🎯 Found {} result(s) for '{}' ({})",
            results.len(),
            query,
            sources.join(" · ")
        )
    }

    /// Format a single result line with source icon, name, and truncated description
    fn format_result_line(&self, index: usize, result: &RegistrySearchResult) -> String {
        let icon = self.get_source_icon(&result.source);
        let stars = if self.config.show_relevance {
            self.get_relevance_stars(result.match_score)
        } else {
            String::new()
        };

        let description = result
            .server_info
            .description
            .as_deref()
            .unwrap_or("No description");
        let truncated_desc = self.truncate_description(description);

        format!(
            "[{}] {} {} {}\n    {}",
            index, icon, result.server_info.name, stars, truncated_desc
        )
    }

    /// Get icon for discovery source
    fn get_source_icon(&self, source: &DiscoverySource) -> &'static str {
        match source {
            DiscoverySource::McpRegistry { .. } => "🔧",
            DiscoverySource::ApisGuru { .. } => "🌐",
            DiscoverySource::WebSearch { .. } => "🔍",
            DiscoverySource::LlmSuggestion { .. } => "🤖",
            DiscoverySource::LocalOverride { .. } => "📂",
            _ => "📦",
        }
    }

    /// Convert match score to star rating (0-3 stars)
    fn get_relevance_stars(&self, score: f32) -> String {
        if score >= 1.0 {
            "⭐⭐⭐".to_string()
        } else if score >= 0.7 {
            "⭐⭐".to_string()
        } else if score >= 0.4 {
            "⭐".to_string()
        } else {
            String::new()
        }
    }

    /// Truncate description to max length, preserving word boundaries
    fn truncate_description(&self, description: &str) -> String {
        // First, clean up the description - take only the first line/sentence
        let clean = description.lines().next().unwrap_or(description).trim();

        // Also stop at first period if it's a reasonable length
        let first_sentence = if let Some(period_pos) = clean.find(". ") {
            if period_pos < self.config.max_description_length {
                &clean[..=period_pos]
            } else {
                clean
            }
        } else {
            clean
        };

        if first_sentence.len() <= self.config.max_description_length {
            first_sentence.to_string()
        } else {
            // Truncate at word boundary
            let truncated = &first_sentence[..self.config.max_description_length];
            if let Some(last_space) = truncated.rfind(' ') {
                format!("{}...", &truncated[..last_space])
            } else {
                format!("{}...", truncated)
            }
        }
    }

    /// Format message for no results
    fn format_no_results(&self, query: &str) -> String {
        let web_status = if crate::discovery::RegistrySearcher::is_web_search_enabled() {
            "✓ Web search enabled"
        } else {
            "ℹ️ Web search disabled (set CCOS_ENABLE_WEB_SEARCH=1 to enable)"
        };

        format!(
            "🔍 No results found for '{}'\n\n\
             Searched:\n\
             • MCP Registry (online)\n\
             • APIs.guru (OpenAPI specs)\n\
             • Local overrides.json\n\
             • {}\n\n\
             💡 Try:\n\
             [1] Different search term\n\
             [2] Configure a custom server\n\
             [3] Synthesize a new capability",
            query, web_status
        )
    }

    // -------------------------------------------------------------------------
    // Server Details Formatting
    // -------------------------------------------------------------------------

    /// Format detailed view of a single server
    pub fn format_server_details(&self, result: &RegistrySearchResult) -> String {
        let mut lines = Vec::new();

        lines.push(format!(
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        ));
        lines.push(format!(
            "{} {}",
            self.get_source_icon(&result.source),
            result.server_info.name
        ));
        lines.push(format!(
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        ));

        // Source info
        lines.push(format!("Source: {}", self.format_source(&result.source)));

        // Endpoint
        if !result.server_info.endpoint.is_empty() {
            lines.push(format!("Endpoint: {}", result.server_info.endpoint));
        }

        // Match score
        lines.push(format!(
            "Relevance: {:.0}% {}",
            result.match_score * 100.0,
            self.get_relevance_stars(result.match_score)
        ));

        // Full description
        if let Some(desc) = &result.server_info.description {
            lines.push(String::new());
            lines.push("Description:".to_string());
            // Wrap description at 70 chars
            for wrapped_line in self.wrap_text(desc, 70) {
                lines.push(format!("  {}", wrapped_line));
            }
        }

        // Alternative endpoints
        if !result.server_info.alternative_endpoints.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "Alternative endpoints ({}):",
                result.server_info.alternative_endpoints.len()
            ));
            for (i, ep) in result.server_info.alternative_endpoints.iter().enumerate() {
                lines.push(format!("  [{}] {}", i + 1, ep));
            }
        }

        // Auth hint
        if let Some(auth_var) = &result.server_info.auth_env_var {
            lines.push(String::new());
            lines.push(format!("🔐 Auth env var: {}", auth_var));
        }

        lines.push(String::new());
        lines.push("💡 'connect' to use this server | 'back' to return to list".to_string());

        lines.join("\n")
    }

    /// Format source for display
    fn format_source(&self, source: &DiscoverySource) -> String {
        match source {
            DiscoverySource::McpRegistry { name } => format!("MCP Registry ({})", name),
            DiscoverySource::ApisGuru { api_name } => format!("APIs.guru ({})", api_name),
            DiscoverySource::WebSearch { url } => format!("Web Search ({})", url),
            DiscoverySource::LlmSuggestion { name } => format!("LLM Suggestion ({})", name),
            DiscoverySource::LocalOverride { path } => format!("Local Override ({})", path),
            _ => "Unknown".to_string(),
        }
    }

    /// Wrap text at specified width
    fn wrap_text(&self, text: &str, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();

        for word in text.split_whitespace() {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_line.len() + 1 + word.len() <= width {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(current_line);
                current_line = word.to_string();
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }
}

impl Default for DialoguePresenter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_description() {
        let presenter = DialoguePresenter::new();
        let result = presenter.truncate_description("A short description");
        assert_eq!(result, "A short description");
    }

    #[test]
    fn test_truncate_long_description() {
        let presenter = DialoguePresenter::with_config(PresenterConfig {
            max_description_length: 20,
            ..Default::default()
        });
        let result = presenter
            .truncate_description("This is a very long description that should be truncated");
        assert!(result.ends_with("..."));
        assert!(result.len() <= 25); // 20 + "..."
    }

    #[test]
    fn test_relevance_stars() {
        let presenter = DialoguePresenter::new();
        assert_eq!(presenter.get_relevance_stars(1.0), "⭐⭐⭐");
        assert_eq!(presenter.get_relevance_stars(0.8), "⭐⭐");
        assert_eq!(presenter.get_relevance_stars(0.5), "⭐");
        assert_eq!(presenter.get_relevance_stars(0.2), "");
    }
}
