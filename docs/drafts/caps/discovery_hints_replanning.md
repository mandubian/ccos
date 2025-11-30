# Discovery Hints & Re-planning

## Problem

When the LLM generates a plan with capabilities that don't exist (e.g., `data.list.filter`), the discovery engine fails and the plan proceeds with stub capabilities. This is inefficient and could be improved by asking the LLM to replan with hints about what capabilities ARE available.

## Current Flow

1. LLM generates plan with steps: `github.issues.list` → `data.list.filter`
2. Discovery finds `github.issues.list` ✅
3. Discovery fails to find `data.list.filter` ❌
4. Plan proceeds with stub for `data.list.filter` (not ideal)

## Proposed Solution: Re-planning with Discovery Hints

### Flow

1. **Discovery Phase**: Try to discover all capabilities in the plan
   - Collect discovered capabilities with their metadata
   - Collect failed capabilities with context
   - Extract hints from discovered capabilities (e.g., parameter support)

2. **Re-planning Trigger**: If any capability fails discovery
   - Build a hint context with:
     - Found capabilities: `github.issues.list` (supports `state`, `labels`, `direction` parameters)
     - Missing capabilities: `data.list.filter`
     - Suggested alternatives: "github.issues.list already supports filtering via state parameter"

3. **Re-planning Request**: Ask arbiter to replan with hints
   ```
   Context:
   - Goal: "filter all issues of github repository ccos of owner mandubian"
   - Found capabilities:
     * github.issues.list (MCP) - Lists issues in a repository
       - Supports filtering via: state (open/closed/all), labels, direction, orderBy
       - For "all issues", use state: "all"
   - Missing capabilities:
     * data.list.filter - Not found in marketplace, MCP, or OpenAPI
   - Suggestion: github.issues.list already supports filtering. Use state: "all" to get all issues.
   
   Please replan using only available capabilities.
   ```

4. **Re-plan**: LLM generates new plan using only available capabilities

## Implementation

### New Discovery Result Type

```rust
pub enum DiscoveryResult {
    Found(CapabilityManifest),
    NotFound,
    Incomplete(CapabilityManifest),
    // New: Discovery hints for re-planning
    DiscoveryHints(DiscoveryHints),
}

pub struct DiscoveryHints {
    pub found_capabilities: Vec<FoundCapability>,
    pub missing_capabilities: Vec<String>,
    pub suggestions: Vec<String>,
}

pub struct FoundCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: String, // "MCP", "OpenAPI", "Local"
    pub parameters: Vec<String>, // Available parameters
    pub hints: Vec<String>, // Usage hints
}
```

### Enhanced Discovery Engine

Add method to collect hints:

```rust
impl DiscoveryEngine {
    /// Collect discovery hints for all capabilities in a plan
    pub async fn collect_discovery_hints(
        &self,
        capabilities: &[String],
    ) -> RuntimeResult<DiscoveryHints> {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        let mut suggestions = Vec::new();
        
        for cap_id in capabilities {
            let need = CapabilityNeed::from_id(cap_id);
            match self.discover_capability(&need).await? {
                DiscoveryResult::Found(manifest) => {
                    // Extract hints from manifest
                    let hints = self.extract_capability_hints(&manifest);
                    found.push(FoundCapability {
                        id: manifest.id.clone(),
                        name: manifest.name.clone(),
                        description: manifest.description.clone(),
                        provider: format!("{:?}", manifest.provider),
                        parameters: extract_parameters(&manifest),
                        hints,
                    });
                }
                _ => {
                    missing.push(cap_id.clone());
                    // Check if there's a related capability that could work
                    if let Some(related) = self.find_related_capability(cap_id).await? {
                        suggestions.push(format!(
                            "{} not found, but {} might work: {}",
                            cap_id, related.id, related.description
                        ));
                    }
                }
            }
        }
        
        Ok(DiscoveryHints {
            found_capabilities: found,
            missing_capabilities: missing,
            suggestions,
        })
    }
    
    fn extract_capability_hints(&self, manifest: &CapabilityManifest) -> Vec<String> {
        let mut hints = Vec::new();
        
        // Extract parameter hints
        if let Some(ref schema) = manifest.input_schema {
            // Parse schema to extract parameter names and types
            // e.g., "state: open|closed|all" -> "Use state: 'all' to get all issues"
        }
        
        // Extract from metadata
        if let Some(desc) = manifest.metadata.get("mcp_tool_description") {
            hints.push(desc.clone());
        }
        
        // Provider-specific hints
        match &manifest.provider {
            ProviderType::MCP(mcp) => {
                hints.push(format!("MCP tool: {}", mcp.tool_name));
                if let Some(url) = manifest.metadata.get("mcp_server_url") {
                    hints.push(format!("Server: {}", url));
                }
            }
            _ => {}
        }
        
        hints
    }
}
```

### Re-planning Integration

In `smart_assistant_demo.rs`:

```rust
async fn resolve_with_replanning(
    ccos: &Arc<CCOS>,
    plan_steps: &[ProposedStep],
    delegating: &DelegatingArbiter,
    goal: &str,
    intent: &Intent,
) -> DemoResult<Vec<ResolvedStep>> {
    // First pass: discover all capabilities
    let discovery_engine = ccos.get_discovery_engine();
    let capability_ids: Vec<String> = plan_steps.iter()
        .map(|s| s.capability_class.clone())
        .collect();
    
    let hints = discovery_engine.collect_discovery_hints(&capability_ids).await?;
    
    // If any capabilities are missing, trigger re-planning
    if !hints.missing_capabilities.is_empty() {
        println!("🔄 Some capabilities not found, asking LLM to replan...");
        
        let replan_prompt = build_replan_prompt(goal, intent, &hints);
        let new_steps = delegating.propose_plan_steps_with_hints(replan_prompt).await?;
        
        // Re-discover with new plan
        return resolve_with_replanning(ccos, &new_steps, delegating, goal, intent).await;
    }
    
    // All capabilities found, proceed with normal resolution
    resolve_and_stub_capabilities(ccos, plan_steps, ...).await
}

fn build_replan_prompt(
    goal: &str,
    intent: &Intent,
    hints: &DiscoveryHints,
) -> String {
    format!(r#"
The previous plan requested capabilities that don't exist. Please replan using only available capabilities.

Goal: {}

Available Capabilities:
{}

Missing Capabilities (not found):
{}

Suggestions:
{}

Please generate a new plan that uses only the available capabilities listed above.
"#,
        goal,
        format_found_capabilities(&hints.found_capabilities),
        hints.missing_capabilities.join(", "),
        hints.suggestions.join("\n")
    )
}
```

## Benefits

1. **More efficient plans**: LLM learns what's actually available
2. **Better capability usage**: Uses full features of available capabilities (e.g., `state: "all"` instead of filtering)
3. **Fewer stub capabilities**: Plans that actually work end-to-end
4. **Learning**: LLM can learn from hints about what capabilities support

## Example

**Before (with stub):**
- Step 1: `github.issues.list` ✅
- Step 2: `data.list.filter` ❌ → stub

**After (with re-planning):**
- Hint: "github.issues.list supports `state: 'all'` parameter to get all issues"
- Re-plan: Single step using `github.issues.list` with `state: "all"` ✅

## Implementation Priority

This fits into "Partial Execution Outcomes" feature (Medium priority). It's a natural extension of the discovery system.























