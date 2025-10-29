# Server Trust and User Interaction Guide

## Overview

The CCOS capability resolution system includes a **Server Trust Registry** with **interactive user prompts** to ensure safe and deliberate selection of MCP servers, especially when dealing with unofficial or unverified sources.

## Trust Levels

The system recognizes four trust levels for MCP servers:

1. **Official** - Officially recognized servers (e.g., github.com, openai.com)
2. **Verified** - Third-party servers that have been verified by the CCOS team
3. **Approved** - Servers approved by the user for their workspace
4. **Unverified** - Unknown servers that require user approval

## User Interaction Flow

### Scenario 1: Unknown Server Detected

When resolving a capability that requires an **unverified server**, the system will prompt for user approval:

```
⚠️  UNKNOWN SERVERS: Found 1 server(s) for 'github.search_code'
These servers are not in the trusted registry:
  1. ai.smithery - Connect AI assistants to your GitHub-hosted Obsidian vault...

Options:
  [1-1] - Select a specific server
  [a] - Approve all servers for future use
  [d] - Deny and cancel resolution

Your choice: 
```

**User Options:**
- **Number (1-N)**: Select a specific server from the list
- **`a`**: Approve ALL listed servers and add them to the user's approved list
- **`d`**: Deny server approval and cancel the capability resolution

### Scenario 2: Multiple Trusted Servers

When multiple trusted servers are available, the system prompts for selection:

```
🔍 MULTIPLE SERVERS: Found 3 server(s) for 'github.issues'
Please select the most appropriate one:

1. github.com ✅ OFFICIAL
   Description: Official GitHub API
   Repository: https://github.com/github/github

2. github-enterprise.com ✅ VERIFIED
   Description: GitHub Enterprise Server
   Repository: https://github.com/enterprise/mcp

3. gitea.io ✅ APPROVED
   Description: Gitea MCP Server
   Repository: https://github.com/gitea/mcp-server

Your choice [1-3]: 
```

**User Options:**
- Enter a number (1-3) to select the desired server

## Trust Policy Configuration

The trust policy can be configured via `ServerTrustRegistry`:

```rust
let policy = TrustPolicy {
    min_auto_select_trust: TrustLevel::Verified,  // Auto-select only Verified+ servers
    prompt_for_unverified: true,                  // Always prompt for unverified
    prompt_for_selection: true,                   // Prompt when multiple options exist
    max_selection_display: 10,                    // Show up to 10 servers in selection
};
```

### Policy Fields

- **`min_auto_select_trust`**: Minimum trust level for automatic selection (no prompt)
- **`prompt_for_unverified`**: If true, always prompt for unverified servers
- **`prompt_for_selection`**: If true, prompt when multiple trusted servers are available
- **`max_selection_display`**: Maximum number of servers to display in selection prompts

## Server Approval Persistence

When a user approves a server:
1. The server is added to the user's `approved_servers` list in the `ServerTrustRegistry`
2. The approval is persisted across sessions (implementation depends on storage backend)
3. Future resolutions using this server will not prompt again (unless policy changes)

## Command-Line Usage

### Interactive Mode (Default)

```bash
cargo run --bin resolve-deps -- resolve --capability-id github.search_code
```

The tool will prompt for user input when unverified servers are encountered.

### Batch Mode (Future)

For CI/CD or automated workflows, a non-interactive mode can be added:

```bash
cargo run --bin resolve-deps -- resolve \
  --capability-id github.search_code \
  --auto-approve \
  --trust-level verified
```

## Security Considerations

1. **Default Deny**: Unknown servers are denied by default unless explicitly approved
2. **User Control**: Users have full control over which servers to trust
3. **Transparency**: Server domain, description, and repository are displayed before approval
4. **Granular Approval**: Users can approve individual servers or all at once
5. **Revocable Trust**: Users can remove servers from the approved list (via future API)

## Implementation Details

### Key Components

- **`ServerTrustRegistry`**: Manages trusted servers and approval state
- **`ServerSelectionHandler`**: Orchestrates user interaction and selection logic
- **`TrustPolicy`**: Configures trust and prompt behavior
- **`ServerCandidate`**: Represents a server option during selection

### User Input Handling

The system uses synchronous stdin reading for user prompts:

```rust
use std::io::{self, Write};

io::stdout().flush()?;
let mut input = String::new();
io::stdin().read_line(&mut input)?;
let choice = input.trim();
```

### Error Handling

- **Invalid Input**: System provides clear error messages and does not proceed
- **Denial**: Returns `RuntimeError::Generic("User denied server approval")`
- **Network Errors**: Falls back gracefully if MCP registry is unavailable

## Future Enhancements

1. **Trust Configuration File**: Persist trust registry to `~/.ccos/trusted_servers.json`
2. **Trust Metrics**: Display download counts, community ratings for unverified servers
3. **Expiration**: Auto-expire approvals after a certain time period
4. **Domain Verification**: Validate server domains against TLS certificates
5. **Audit Log**: Track all server approvals and denials for security audits

## Related Documentation

- [MCP Runtime Guide](./mcp-runtime-guide.md) - How MCP capabilities work at runtime
- [MCP Synthesis Guide](./mcp-synthesis-guide.md) - How MCP capabilities are generated
- [Metadata-Driven Capabilities](./metadata-driven-capabilities.md) - Capability metadata structure


