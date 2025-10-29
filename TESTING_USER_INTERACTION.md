# Testing User Interaction for Server Trust

## ✅ Fixed: Interactive User Prompts with Multiple Servers

The server trust system now properly **waits for user input** and handles the common case of **43+ servers** by showing only the top 10 and allowing refinement.

### What Was Fixed

**Before:**
- System displayed a prompt but immediately auto-selected the first server
- Showed only 1 server even when 43 were found
- No actual user interaction occurred

**After:**
- System displays top 10 discovered servers (by relevance score)
- Waits for user input via stdin
- Allows viewing all servers with 'm' (more)
- Allows refining search with 'r' (refine) + hint
- Validates user choice and proceeds accordingly

### How to Test

1. **Run the resolve-deps tool:**
   ```bash
   cd rtfs_compiler
   cargo run --bin resolve-deps -- resolve --capability-id github.search_code
   ```

2. **You will see a prompt like this (showing top 10 of 43 servers):**
   ```
   ⚠️  UNKNOWN SERVERS: Found 43 server(s) for 'github.search_code'
   These servers are not in the trusted registry.

   Showing top 10 ranked by relevance:

     1. ai.smithery - Connect AI assistants to your GitHub-hosted Obsidian vault...
        Repository: https://github.com/hint-services/obsidian-github-mcp
        Score: 0.85

     2. github.example - Example GitHub MCP server
        Repository: https://github.com/example/mcp
        Score: 0.72

     3. gitea.io - Gitea MCP integration
        Repository: https://github.com/gitea/mcp-server
        Score: 0.68

     ... (7 more shown)

   ... and 33 more server(s) not shown

   Enter a number (1-10) to select a server
   Enter 'a' to approve all 43 servers for future use
   Enter 'm' to see more servers
   Enter 'r' to refine your search with a hint
   Enter 'd' to deny and cancel resolution

   Your choice: 
   ```

3. **Test Different Inputs:**

   - **Enter `1`**: Selects server #1 (ai.smithery)
     ```
     ✅ User selected server: ai.smithery
     ```

   - **Enter `2`**: Selects server #2 (github.example)
     ```
     ✅ User selected server: github.example
     ```

   - **Enter `a`**: Approves ALL servers and selects the first one
     ```
     ✅ User approved all servers for future use.
     ✅ Selected server: ai.smithery
     ```

   - **Enter `d`**: Denies all servers and cancels resolution
     ```
     ❌ User denied server approval. Resolution cancelled.
     Error: Generic("User denied server approval")
     ```

   - **Enter `m`**: Shows ALL 43 servers with full details
     ```
     📋 All 43 servers:

       1. ai.smithery - Connect AI assistants...
       2. github.example - Example GitHub MCP...
       ... (all 43 shown)
     
     Enter a number (1-43) to select a server
     Your choice:
     ```

   - **Enter `r`**: Refine search with a hint
     ```
     🔍 Enter a search hint to filter servers (e.g., 'github', 'obsidian', 'official'):
     Hint: github
     
     ✅ Found 12 server(s) matching 'github'
     
     (Shows only the 12 servers containing "github" in name/description/domain)
     ```

   - **Enter invalid input** (e.g., `999` or `xyz`): Shows error
     ```
     ❌ Invalid input: 'xyz'. Expected a number (1-10), 'a', 'm', 'r', or 'd'
     Error: Generic("Invalid input: xyz")
     ```

### Key Improvements

1. **Clear Server Display:**
   - Each server is numbered (1, 2, 3...)
   - Shows domain, description, repository, and relevance score
   - Easy to compare and choose

2. **Better Instructions:**
   - Clear options: number selection, approve all, or deny
   - No ambiguous `[1-N]` notation

3. **Robust Input Handling:**
   - Validates numeric range (1 to N)
   - Handles special commands ('a', 'd')
   - Provides clear error messages for invalid input

4. **Session Persistence:**
   - Approved servers are saved to the trust registry
   - Future resolutions won't prompt again for approved servers

### Testing Multiple Servers Scenario

To test with servers that have different trust levels, you can:

1. First run: Select a server and approve it
2. Second run: The approved server won't prompt anymore
3. Add more servers to the trust registry programmatically
4. Test the multi-server selection prompt

### Expected Behavior

- **Unverified servers**: Always prompt for approval
- **Approved servers**: Skip prompt, auto-select
- **Official servers**: Skip prompt, auto-select
- **Multiple trusted servers**: Show selection prompt (if policy enabled)

### Configuration

The behavior can be customized via `TrustPolicy`:

```rust
let policy = TrustPolicy {
    min_auto_select_trust: TrustLevel::Verified,  // Auto-select Verified+ servers
    prompt_for_unverified: true,                  // Always prompt for unverified
    prompt_for_selection: true,                   // Prompt when multiple options
    max_selection_display: 10,                    // Show up to 10 servers
};
```

### Related Documentation

- [Server Trust User Interaction Guide](./docs/ccos/guides/server-trust-user-interaction.md)
- [MCP Synthesis Guide](./docs/ccos/guides/mcp-synthesis-guide.md)

