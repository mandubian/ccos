# GitHub MCP Capability Demo

This demo showcases the GitHub MCP (Model Context Protocol) capability, which allows RTFS programs to interact with GitHub's API for issue management.

## Overview

The GitHub MCP capability provides three main functions:
- **List Issues**: Retrieve issues from a GitHub repository
- **Create Issue**: Create new issues with titles, bodies, labels, and assignees
- **Close Issue**: Close existing issues with optional comments

## Features

- ✅ **Full GitHub API Integration**: Uses GitHub REST API v3
- ✅ **Authentication Support**: Supports GitHub personal access tokens
- ✅ **Error Handling**: Comprehensive error handling and validation
- ✅ **Type Safety**: Full type checking for inputs and outputs
- ✅ **RTFS Integration**: Seamless integration with RTFS programs
- ✅ **MCP Protocol Compliance**: Follows Model Context Protocol standards

## Setup

### 1. GitHub Token

You'll need a GitHub personal access token with appropriate permissions:

1. Go to GitHub Settings → Developer settings → Personal access tokens
2. Generate a new token with these scopes:
   - `repo` (for private repositories)
   - `public_repo` (for public repositories)

### 2. Environment Setup

Set your GitHub token as an environment variable:

```bash
export GITHUB_TOKEN="your_github_token_here"
```

## Usage

### Rust Demo

Run the Rust demo program:

```bash
# List issues
cargo run --example github_mcp_demo -- --action list

# Create an issue
cargo run --example github_mcp_demo -- --action create \
  --title "Test Issue" \
  --body "This is a test issue created via MCP"

# Close an issue
cargo run --example github_mcp_demo -- --action close --issue-number 123
```

### RTFS Demo

Run the RTFS demo program:

```bash
# Set your GitHub token
export GITHUB_TOKEN="your_github_token_here"

# Run the RTFS program
cargo run --bin rtfs_compiler examples/github_mcp_demo.rtfs
```

## API Reference

### List Issues

```rtfs
(call :github_mcp.list_issues {
  :owner "repository_owner"
  :repo "repository_name"
  :state "open"           ; "open", "closed", or "all"
  :per_page 30           ; number of issues per page
  :page 1                ; page number
})
```

**Response:**
```json
{
  "success": true,
  "issues": [...],
  "total_count": 42
}
```

### Create Issue

```rtfs
(call :github_mcp.create_issue {
  :owner "repository_owner"
  :repo "repository_name"
  :title "Issue Title"
  :body "Issue description"
  :labels ["label1" "label2"]
  :assignees ["username1" "username2"]
})
```

**Response:**
```json
{
  "success": true,
  "issue": {...},
  "issue_number": 123,
  "html_url": "https://github.com/owner/repo/issues/123"
}
```

### Close Issue

```rtfs
(call :github_mcp.close_issue {
  :owner "repository_owner"
  :repo "repository_name"
  :issue_number 123
  :comment "Optional closing comment"
})
```

**Response:**
```json
{
  "success": true,
  "issue": {...},
  "message": "Issue #123 closed successfully"
}
```

## Integration with CCOS

The GitHub MCP capability integrates seamlessly with the CCOS capability system:

### Registration

```rust
use rtfs_compiler::capabilities::GitHubMCPCapability;

let capability = GitHubMCPCapability::new(Some(github_token));
// Register with capability marketplace
```

### Security

The capability includes built-in security features:
- Network access restrictions (only `api.github.com`)
- Authentication requirements
- Input validation and sanitization
- Error handling and logging

### Health Monitoring

```rust
let health = capability.health_check();
println!("Health Status: {:?}", health);
```

## Error Handling

The capability provides comprehensive error handling:

```rtfs
(try 
  (call :github_mcp.create_issue {...})
  (catch error 
    (println "Failed to create issue:" error)))
```

Common error scenarios:
- **Authentication errors**: Invalid or expired token
- **Permission errors**: Insufficient repository access
- **Validation errors**: Missing required fields
- **Network errors**: Connection issues

## Examples

### Automated Issue Management

```rtfs
;; Create a comprehensive issue management workflow
(step "Issue Management Workflow"
  (let [issues (call :github_mcp.list_issues {
    :owner "myorg"
    :repo "myproject"
    :state "open"
  })]
    (step "Process Each Issue"
      (for [issue (:issues issues)]
        (step (str "Process Issue #" (:number issue))
          (if (= (:priority issue) "high")
            (call :github_mcp.create_issue {
              :owner "myorg"
              :repo "myproject"
              :title (str "Follow-up: " (:title issue))
              :body "High priority issue requires immediate attention"
              :labels ["high-priority" "follow-up"]
            })
            (println "Issue processed normally")))))))
```

### Issue Templates

```rtfs
;; Create issues from templates
(defn create-bug-report [title description severity]
  (call :github_mcp.create_issue {
    :owner "myorg"
    :repo "myproject"
    :title title
    :body (str "## Bug Report\n\n" description "\n\n**Severity:** " severity)
    :labels ["bug" severity]
    :assignees ["qa-team"]
  }))

(step "Create Bug Report"
  (create-bug-report 
    "Login page not working"
    "Users cannot log in after the latest deployment"
    "high"))
```

## Testing

Run the test suite:

```bash
cargo test github_mcp
```

The tests cover:
- ✅ Capability creation and initialization
- ✅ Tool discovery and registration
- ✅ Value conversion (RTFS ↔ JSON)
- ✅ Error handling scenarios
- ✅ Provider interface compliance

## Troubleshooting

### Common Issues

1. **"Bad credentials" error**
   - Check that your GitHub token is valid and has the correct permissions
   - Ensure the token hasn't expired

2. **"Not found" error**
   - Verify the repository owner and name are correct
   - Check that you have access to the repository

3. **"Validation failed" error**
   - Ensure all required fields are provided
   - Check that field values match expected formats

### Debug Mode

Enable debug logging:

```bash
RUST_LOG=debug cargo run --example github_mcp_demo
```

## Future Enhancements

Planned features for future versions:
- [ ] Pull request management
- [ ] Repository management
- [ ] Webhook integration
- [ ] Batch operations
- [ ] Rate limiting and caching
- [ ] GraphQL API support

## Contributing

To contribute to the GitHub MCP capability:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass
5. Submit a pull request

## License

This capability is part of the CCOS project and follows the same license terms. 