# MCP Server Relevance Scoring

## Why Only 1 Server from 43?

When you see:
```
🔍 DISCOVERY: Found 43 MCP servers for 'github.search_code'
⚠️  UNKNOWN SERVERS: Found 1 server(s) for 'github.search_code'
(Only 1 server scored >= 0.3 relevance threshold from initial discovery)
```

This is **expected and good**! The system uses intelligent filtering to avoid overwhelming you with irrelevant servers.

## How Server Scoring Works

### Step 1: Initial Discovery
- Query MCP Registry with your capability query (e.g., "github.search_code")
- Receive ALL matching servers (e.g., 43 servers)

### Step 2: Relevance Scoring
Each server is scored (0.0 to 10.0+) based on how well it matches your query:

**Scoring Algorithm** (`calculate_server_score`):
- **Exact name match**: +10.0 points (highest priority)
- **Exact match in description**: +8.0 points
- **Partial name match**: +6.0 points
- **Reverse partial match**: +4.0 points (query contains server name)
- **Description contains capability**: +3.0 points

**Example for "github.search_code":**
- Server: `ai.smithery/Hint-Services-obsidian-github-mcp`
  - Name contains "github": +6.0 (partial match)
  - Description: "Connect AI assistants to your GitHub-hosted Obsidian vault..."
  - Score: ~6.0-8.0 ✅ **Passes threshold**

- Server: `random.mcp.server`
  - Name: no match (0.0)
  - Description: "Some random MCP functionality"
  - Score: 0.0 ❌ **Filtered out**

### Step 3: Threshold Filtering

**Default Threshold: 0.3**

Only servers with `score >= 0.3` are shown to the user. This filters out:
- Servers with no relevance to your query
- Generic MCP servers that happen to match keywords
- Servers with very weak/coincidental matches

**Result**: Out of 43 servers, only 1-5 truly relevant servers are shown.

## Configuration

### Adjusting the Threshold

The threshold is defined in `missing_capability_resolver.rs`:

```rust
// Filter out servers with very low scores
ranked
    .into_iter()
    .filter(|ranked| ranked.score >= 0.3) // Minimum threshold
    .collect()
```

**To show more servers**, you can:

1. **Lower the threshold** (e.g., `>= 0.1`):
   - Shows more servers, including weakly related ones
   - May overwhelm users with less relevant options

2. **Keep no threshold** (comment out filter):
   - Shows all 43 servers
   - Relies entirely on user's ability to choose

3. **Make it configurable** (recommended):
   ```rust
   .filter(|ranked| ranked.score >= self.config.min_server_score)
   ```

### Verbose Logging

Run with verbose mode to see scoring details:
```bash
cargo run --bin resolve-deps -- resolve --capability-id github.search_code --verbose
```

This will show:
```
🔍 DISCOVERY: Found 43 MCP servers for 'github.search_code'
📊 DISCOVERY: Ranked 1 server(s) with score >= 0.3 for 'github.search_code'
   1. ai.smithery/Hint-Services-obsidian-github-mcp (score: 6.50)
```

## User Options When Only 1 Server is Found

Even with 1 server, users have full control:

1. **Select it** (enter `1`):
   - Approves and uses this server
   - Adds it to trusted registry

2. **Deny it** (enter `d`):
   - Cancels resolution
   - Can refine query and try again with a different search term

3. **Approve all** (enter `a`):
   - Approves this server (and any others if present)

### ⚠️ Note on "Refine Search" Option

The `r` (refine) option **only appears when multiple servers are shown**. When only 1 server is found:
- The refine option is **hidden** (not useful)
- The 42 other servers were **already filtered out** by relevance score
- They're not available to refine because they scored < 0.3

**To see ALL servers (including low-relevance ones):**

Currently, you need to:
1. **Lower the threshold** in the code (see "Configuration" section)
2. **Use a different search query** that might match different servers
3. **Browse the MCP Registry directly** to see all available servers

## Design Rationale

### Why Filter at All?

**Without filtering (showing all 43 servers):**
- ❌ Overwhelming for users
- ❌ Difficult to identify the best option
- ❌ Many irrelevant servers waste time

**With score-based filtering:**
- ✅ Shows only relevant servers
- ✅ User can focus on quality matches
- ✅ Faster decision-making
- ✅ Still allows user to deny if not satisfied

### Why 0.3 Threshold?

- **Too low (0.0)**: No filtering benefit
- **Too high (1.0+)**: Only exact matches shown
- **0.3 (current)**: Balances precision and recall
  - Catches partial matches (+3.0 to +6.0 scores)
  - Filters out coincidental matches (<0.3)

## Future Enhancements

1. **Configurable Threshold**:
   ```rust
   pub struct ResolverConfig {
       pub min_server_score: f64,  // Default: 0.3
       // ...
   }
   ```

2. **Show Filtered Count**:
   ```
   ⚠️  UNKNOWN SERVERS: Found 1 server(s) for 'github.search_code'
   (Filtered from 43 total servers based on relevance score)
   Enter 's' to show all servers (including low-relevance ones)
   ```

3. **Adaptive Threshold**:
   - If 0 servers pass 0.3, lower to 0.1
   - If >20 servers pass, raise to 0.5
   - Ensures user always has options

4. **Score Display**:
   - Show relevance score next to each server
   - Help users understand why servers were selected

## Related Documentation

- [Server Trust User Interaction Guide](./server-trust-user-interaction.md)
- [MCP Synthesis Guide](./mcp-synthesis-guide.md)

