# MCP Discovery Real-Life Example

## Overview

This example demonstrates **description-based semantic matching** for discovering GitHub capabilities via MCP introspection. It shows how functional descriptions like "List issues in a GitHub repository" can be matched against MCP capability descriptions.

## Purpose

- **Test description-based matching in isolation** from the full discovery pipeline
- **See how MCP introspection works** with semantic search
- **Verify rationale generation improvements** work with real MCP servers
- **Demonstrate discovery without execution** (pure service discovery)

## What It Tests

### Test Case 1: Functional Description → MCP Discovery
- Creates a `CapabilityNeed` with a functional rationale: "List all open issues in a GitHub repository"
- Searches MCP registry using description-based matching
- Shows how the rationale matches MCP capability descriptions semantically

### Test Case 2: Wording Variations
- Tests different ways of expressing the same need:
  - "List all open issues in a GitHub repository" (high confidence)
  - "List issues in a GitHub repository" (high confidence)
  - "Retrieve GitHub repository issues" (medium confidence)
  - "Get issues from GitHub repo" (medium confidence)
  - "Need to see all issues in my GitHub repo" (lower confidence)

## Usage

```bash
cargo run --example test_mcp_discovery
```

## Expected Output

```
🔍 Real-Life Example: GitHub Issue Discovery via MCP

════════════════════════════════════════════════════════════════════

📋 Test Case 1: Functional Description to MCP Discovery
────────────────────────────────────────────────────────────────────

Goal: Find a capability to list GitHub repository issues
Using functional description (what we need):
  'List all open issues in a GitHub repository'

CapabilityNeed:
  Class: github.issues.list
  Rationale: List all open issues in a GitHub repository
  Inputs: ["repository", "state"]
  Outputs: ["issues_list"]

🔍 Searching MCP registry...
  (This will introspect MCP servers and match by description)

✅ FOUND via MCP introspection:
   ID: mcp.github.list_issues
   Name: list_issues
   Description: List issues in a GitHub repository. For pagination...
   
   📊 Match Details:
      • LLM generated: github.issues.list
      • Found: mcp.github.list_issues
      • Rationale matched description semantically
```

## How It Works

1. **Setup**: Creates a minimal CCOS instance with marketplace and intent graph
2. **Create Need**: Builds a `CapabilityNeed` with a functional rationale (not generic step name)
3. **MCP Search**: Calls `discovery_engine.search_mcp_registry()` which:
   - Searches MCP registry for matching servers (by keywords)
   - Introspects each server to discover tools
   - Uses `calculate_description_match_score()` to match rationale → capability description
   - Returns the best matching capability manifest
4. **Display Results**: Shows the discovered capability and match details

## Key Features Demonstrated

✅ **Description-based matching**: Rationale "List issues in a GitHub repository" matches MCP description semantically  
✅ **MCP introspection**: Discovers capabilities from MCP servers dynamically  
✅ **Functional rationale**: Uses descriptive text instead of generic "Need for step: X"  
✅ **Pure discovery**: No execution, just capability discovery  

## Prerequisites

- MCP servers configured (GitHub MCP server should be available)
- The MCP registry should be accessible
- Runtime environment should allow network access for MCP introspection

## Alternative: Direct Server Introspection

If the MCP registry doesn't have the server, use `test_mcp_discovery_direct.rs`:

```bash
export GITHUB_MCP_URL="https://api.githubcopilot.com/mcp/"
export GITHUB_TOKEN="your_github_token"
cargo run --example test_mcp_discovery_direct
```

This bypasses the registry and directly introspects a known MCP server URL with authentication.

## Notes

- This example focuses **only on discovery**, not execution
- It demonstrates the **description-based semantic matching** improvements
- Shows how **improved rationale generation** helps discovery accuracy
- Can be used to test different rationale formats and their matching scores

## Related

- `test_capability_matching.rs` - Unit tests for matching algorithms
- `smart_assistant_demo.rs` - Full demo with discovery + execution

