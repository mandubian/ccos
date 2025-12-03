# [CLI UX] Add Interactive Mode and Improve Discovery Filtering

## Overview

Improve CLI usability for capability discovery workflow by adding interactive selection modes and better semantic filtering. This enables more efficient human-in-the-loop workflows while preserving scriptable independent commands.

## Motivation

Current `discover goal "list issues"` dumps all matches to `pending.json` without:
1. **Filtering by relevance** - Returns 21 unrelated APIs (trakt.tv, bunq.com, AWS services) for "list issues"
2. **User selection** - No way to pick which results to queue
3. **Context retention** - Must copy-paste IDs between `approval pending` and `approval approve`

This creates friction in the discovery → approval → use workflow.

## Context: Autonomous Agent Pipeline

The CLI is the **human interface** to the same capabilities used by autonomous agents:
- CLI commands are exposed as `ccos.cli.*` native capabilities (Phase 6, #173)
- Improved CLI filtering directly benefits the autonomous agent's discovery logic
- Interactive mode helps humans refine what agents will later do autonomously

```
Human (CLI) ──┐
              ├──► Discovery/Approval Engine ──► Capability Marketplace
Agent (RTFS) ─┘
```

## Proposed Changes

### 1. Add `--interactive` / `-i` Flag

```bash
# Interactive discovery - show ranked results, let user select
ccos discover goal "list issues" --interactive

# Output:
# 🔍 Searching for capabilities matching "list issues"...
# 
# Found 5 relevant results (filtered from 21 candidates):
#
#   [1] ★★★★★ github-mcp          GitHub Issues, PRs, repos
#   [2] ★★★★☆ linear-api          Linear issue tracking  
#   [3] ★★★☆☆ jira-api            Jira/Atlassian issues
#   [4] ★★☆☆☆ gitlab-api          GitLab issues
#   [5] ★☆☆☆☆ azure-devops        Azure DevOps work items
#
# Select [1,3,5 | 1-3 | all | none | q]: 1,2
# ✅ Queued 2 servers for approval: github-mcp, linear-api

# Interactive approval - select from list
ccos approval pending --interactive

# Output:
#   [1] github-mcp (WebSearch) - awaiting approval
#   [2] linear-api (ApisGuru) - awaiting approval
#
# Action [approve/reject] [1,2 | all]: approve 1
# ✅ Approved github-mcp. Discovering 15 capabilities...
```

### 2. Wire Up Existing Scoring Infrastructure

**Problem**: The `discover goal` flow doesn't use the existing scoring in `capability_matcher.rs`.

**Root Cause Analysis**:
```
Current flow:
  GoalDiscoveryAgent.process_goal()
    └── registry_searcher.search()  → Returns ALL with hardcoded scores (1.0, 0.8)
    └── Adds ALL to queue           → No filtering, no actual scoring

What exists but isn't used:
  capability_matcher.rs:
    - extract_domain_keywords()           → Generic keyword extraction
    - calculate_domain_mismatch_penalty() → 0.8 penalty if domains don't overlap
    - calculate_description_match_score_improved() → Combines action verbs + domain
```

**Solution**: Wire up existing generic infrastructure (NO hardcoded domains):

```rust
// In goal_discovery.rs or registry_search.rs
use crate::discovery::capability_matcher::calculate_description_match_score_improved;

// For each search result, compute actual score:
let score = calculate_description_match_score_improved(
    goal,                           // "list issues"  
    &result.server_info.description, // Server description
    &result.server_info.name,        // Server name
    "",                              // capability_class (optional)
    &result.server_info.name,        // manifest_id
    &config,                         // DiscoveryConfig
);

// Only add if above threshold
if score >= config.min_discovery_score {
    approval_queue.add(discovery)?;
}
```

**Why this is generic** (no hardcoded domains):
- `extract_domain_keywords()` extracts context words from ANY text
- "list issues" → extracts ["issues"] as domain keyword
- trakt.tv description → extracts ["tv", "shows", "movies", "media"]  
- No overlap → 0.8 penalty → low score → filtered out
- GitHub description → extracts ["github", "issues", "repository"]
- Overlap on "issues" → no penalty → high score → included

**Configurable thresholds** (in `DiscoveryConfig` or TOML):
```toml
[discovery]
min_score_threshold = 0.5
top_k_results = 10
action_verb_weight = 0.4
domain_mismatch_penalty = 0.8
```

### 3. Enhanced REPL Mode (Optional, Future)

Extend existing `explore` REPL to support discovery/approval workflow with session context:

```
ccos explore
ccos> discover "list issues"
Found 3 matches. [1] github-mcp [2] linear-api [3] jira-api

ccos> select 1
Selected: github-mcp

ccos> approve
✅ Approved. Discovered 15 capabilities.

ccos> call github.list_issues owner:mandubian repo:ccos
[... results ...]
```

## Implementation Plan

### Phase 1: Wire Up Existing Scoring (Priority)
- [ ] In `goal_discovery.rs`: Call `calculate_description_match_score_improved()` for each result
- [ ] Apply `config.min_discovery_score` threshold (filter out low scores)
- [ ] Sort results by score descending before adding to queue
- [ ] Add `--top N` and `--threshold SCORE` CLI flags
- [ ] Display scores in output (star rating or numeric)

### Phase 2: Interactive Flags
- [ ] Add `--interactive` / `-i` flag to `discover goal`
- [ ] Add `--interactive` / `-i` flag to `approval pending`  
- [ ] Implement numbered selection parser (1,3,5 | 1-3 | all)
- [ ] When score variance is high, auto-suggest interactive mode

### Phase 3: Context & History (Future)
- [ ] Track user approval history in working memory
- [ ] Use `discovery_agent.rs` context_vocab for learned preferences
- [ ] Weight previously-approved server domains higher
- [ ] Auto-suggest based on recent activity

## CLI Changes Summary

```
# New flags
ccos discover goal <GOAL> [--interactive|-i] [--top N] [--threshold SCORE]
ccos approval pending [--interactive|-i]

# Behavior
--interactive: Show numbered list, prompt for selection
--top N: Show top N results (default: 10)
--threshold: Minimum score to include (default: 0.3)
```

## Acceptance Criteria

- [ ] `discover goal "list issues" -i` shows ranked, numbered results
- [ ] User can select by number (1,3), range (1-3), or keywords (all, none)
- [ ] `approval pending -i` allows inline approve/reject
- [ ] Score considers domain relevance, not just keyword overlap
- [ ] Ambiguous queries return fewer, more relevant results
- [ ] Independent (non-interactive) commands still work for scripting

## Related

- #174 - This issue
- #173 - Phase 6: Expose CLI as Governed Native Capabilities
- #167 - [Umbrella] CCOS CLI: Unified Command-Line Tool
- Design doc: `docs/drafts/ccos-cli-unified-tool.md`

