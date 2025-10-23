# Capability Metadata Structure

## Overview

RTFS capabilities use a **hierarchical metadata structure** for provider-specific information and discovery metadata. This provides better organization, namespacing, and extensibility.

## RTFS Format Structure

### Generic Capability Template
```clojure
(capability "<capability-id>"
  :name "<name>"
  :version "<version>"
  :description "<description>"
  :provider "<provider>"
  :permissions [...]
  :effects [...]
  :metadata {
    :<provider-type> {
      ;; Provider-specific metadata
    }
    :discovery {
      ;; Discovery metadata
    }
  }
  :input-schema {...}
  :output-schema {...}
  :implementation ...)
```

### Benefits of Hierarchical Structure

1. **Namespaced** - No conflicts between provider-specific fields
2. **Extensible** - Easy to add new provider types
3. **Self-documenting** - Clear what metadata belongs to what
4. **Runtime-friendly** - Easy to navigate: `(get-in metadata [:mcp :server_url])`
5. **Generic** - Works for any provider (MCP, OpenAPI, GraphQL, etc.)

## Provider-Specific Metadata

### MCP Capabilities

```clojure
:metadata {
  :mcp {
    :server_url "https://api.githubcopilot.com/mcp/"
    :server_name "github"
    :tool_name "list_issues"
    :protocol_version "2024-11-05"
    :requires_session "auto"              ; "auto" | "true" | "false"
    :auth_env_var "MCP_AUTH_TOKEN"        ; Generic auth token env var
    :server_url_override_env "MCP_SERVER_URL"  ; URL override env var
  }
  :discovery {
    :method "mcp_introspection"
    :source_url "https://api.githubcopilot.com/mcp/"
    :created_at "2025-10-23T21:35:43Z"
    :capability_type "mcp_tool"
  }
}
```

**Key Fields:**
- `server_url` - Default MCP server URL
- `server_name` - Server namespace (e.g., "github", "slack")
- `tool_name` - MCP tool name
- `protocol_version` - MCP protocol version
- `requires_session` - Session management hint for runtime
- `auth_env_var` - Generic environment variable for auth token
- `server_url_override_env` - Environment variable to override server URL

### OpenAPI Capabilities

```clojure
:metadata {
  :openapi {
    :base_url "https://api.openweathermap.org"
    :endpoint_method "GET"
    :endpoint_path "/data/2.5/forecast"
    :api_version "2.5"
    :auth_type "api_key"                  ; "api_key" | "bearer" | "oauth2"
    :auth_location "query"                ; "query" | "header" | "body"
    :auth_param_name "appid"              ; Parameter name for auth
  }
  :discovery {
    :method "api_introspection"
    :source_url "https://api.openweathermap.org"
    :created_at "2025-10-23T21:35:44Z"
    :capability_type "specialized_http_api"
  }
}
```

**Key Fields:**
- `base_url` - API base URL
- `endpoint_method` - HTTP method (GET, POST, etc.)
- `endpoint_path` - Endpoint path
- `api_version` - API version
- `auth_type` - Authentication mechanism
- `auth_location` - Where auth goes (query param, header, etc.)
- `auth_param_name` - Parameter name for auth credential

### GraphQL Capabilities (Future)

```clojure
:metadata {
  :graphql {
    :endpoint_url "https://api.github.com/graphql"
    :operation_type "query"               ; "query" | "mutation" | "subscription"
    :operation_name "GetRepository"
    :schema_hash "abc123..."              ; Hash of GraphQL schema
    :requires_introspection true
  }
  :discovery {
    :method "graphql_introspection"
    :source_url "https://api.github.com/graphql"
    :created_at "2025-10-23T21:35:44Z"
    :capability_type "graphql_operation"
  }
}
```

## Discovery Metadata

Common across all capability types:

```clojure
:discovery {
  :method "mcp_introspection"              ; How capability was discovered
  :source_url "https://..."                ; Source API/server URL
  :created_at "2025-10-23T21:35:43Z"      ; Timestamp of generation
  :capability_type "mcp_tool"              ; Type of capability
  :introspector_version "1.0.0"           ; Version of introspector (optional)
}
```

**Key Fields:**
- `method` - Discovery method (mcp_introspection, api_introspection, graphql_introspection, manual, etc.)
- `source_url` - Original source URL
- `created_at` - ISO 8601 timestamp
- `capability_type` - Type of capability for filtering/categorization

## Runtime Access Patterns

### RTFS Code
```clojure
;; Get MCP server URL with fallback
(let [mcp-server-url (or 
                       (call "ccos.system.get-env" "MCP_SERVER_URL")
                       (get-in capability-metadata [:mcp :server_url])
                       "http://localhost:3000/mcp")]
  ...)

;; Check if session management required
(let [requires-session (get-in capability-metadata [:mcp :requires_session])
      use-sessions? (or (= requires-session "true")
                        (= requires-session "auto"))]
  ...)

;; Get auth environment variable name
(let [auth-env-var (get-in capability-metadata [:mcp :auth_env_var] "MCP_AUTH_TOKEN")
      token (call "ccos.system.get-env" auth-env-var)]
  ...)
```

### Rust Code (Internal)
```rust
// Access flat structure
let server_url = capability.metadata.get("mcp_server_url");
let requires_session = capability.metadata.get("mcp_requires_session");

// Check session requirement
if requires_session == Some("auto") || requires_session == Some("true") {
    // Use session management
}
```

## Migration from Flat Structure

### Old Format (❌ Deprecated)
```clojure
(capability "mcp.github.list_issues"
  :name "list_issues"
  :source_url "https://api.githubcopilot.com/mcp/"
  :discovery_method "mcp_introspection"
  :created_at "2025-10-23T20:57:22Z"
  :capability_type "mcp_tool"
  :mcp_metadata {                         ; Still flat, just renamed
    :server_url "..."
    :server_name "..."
    :tool_name "..."
  }
  :mcp_requires_session "auto"            ; Top-level field
  :mcp_auth_env_var "MCP_AUTH_TOKEN"      ; Top-level field
  ...)
```

### New Format (✅ Current)
```clojure
(capability "mcp.github.list_issues"
  :name "list_issues"
  :metadata {
    :mcp {
      :server_url "..."
      :server_name "..."
      :tool_name "..."
      :requires_session "auto"
      :auth_env_var "MCP_AUTH_TOKEN"
    }
    :discovery {
      :method "mcp_introspection"
      :source_url "https://api.githubcopilot.com/mcp/"
      :created_at "2025-10-23T20:57:22Z"
    }
  }
  ...)
```

**Key Improvements:**
- ✅ Provider-specific fields under `:metadata { :<provider> }` namespace
- ✅ Discovery info consolidated under `:metadata { :discovery }`
- ✅ No top-level pollution with provider-specific fields
- ✅ Clear separation of concerns
- ✅ Extensible for new providers

## Design Principles

### 1. **Generic at Top Level**
Top-level fields work for **all** capability types:
- `:name`, `:version`, `:description`
- `:provider`, `:permissions`, `:effects`
- `:input-schema`, `:output-schema`
- `:implementation`

### 2. **Provider-Specific in Metadata**
Provider-specific details go under `:metadata { :<provider> }`:
- MCP: session management, server info
- OpenAPI: endpoint details, auth config
- GraphQL: operation type, schema hash

### 3. **Discovery Info Separate**
Discovery metadata in `:metadata { :discovery }`:
- How was it discovered?
- When was it generated?
- From what source?

### 4. **Runtime Hints, Not Implementation**
Metadata provides **hints** to the runtime:
- `requires_session "auto"` → runtime decides
- `auth_env_var "MCP_AUTH_TOKEN"` → suggests env var
- Implementation reads these hints, not hardcoded

## Examples

### Complete MCP Capability
```clojure
(capability "mcp.github.list_issues"
  :name "list_issues"
  :version "1.0.0"
  :description "List issues in a GitHub repository"
  :provider "MCP"
  :permissions [:network.http]
  :effects [:network_request :mcp_call]
  :metadata {
    :mcp {
      :server_url "https://api.githubcopilot.com/mcp/"
      :server_name "github"
      :tool_name "list_issues"
      :protocol_version "2024-11-05"
      :requires_session "auto"
      :auth_env_var "MCP_AUTH_TOKEN"
      :server_url_override_env "MCP_SERVER_URL"
    }
    :discovery {
      :method "mcp_introspection"
      :source_url "https://api.githubcopilot.com/mcp/"
      :created_at "2025-10-23T21:35:43Z"
      :capability_type "mcp_tool"
    }
  }
  :input-schema {
    :owner :string
    :repo :string
    :state :string ;; optional
  }
  :output-schema [:vector :map]
  :implementation
    (fn [input] ...))
```

### Complete OpenAPI Capability
```clojure
(capability "openweather_api.get_forecast"
  :name "Get 5 Day Weather Forecast"
  :version "2.5"
  :description "5 day weather forecast with data every 3 hours"
  :provider "OpenWeather API"
  :permissions [:network.http]
  :effects [:network_request :auth_required]
  :metadata {
    :openapi {
      :base_url "https://api.openweathermap.org"
      :endpoint_method "GET"
      :endpoint_path "/data/2.5/forecast"
    }
    :discovery {
      :method "api_introspection"
      :source_url "https://api.openweathermap.org"
      :created_at "2025-10-23T21:35:44Z"
      :capability_type "specialized_http_api"
    }
  }
  :input-schema {
    :q :string ;; optional
    :lat :float ;; optional
    :lon :float ;; optional
  }
  :output-schema {
    :cnt :int
    :list [:vector :map]
  }
  :implementation
    (fn [input] ...))
```

## Benefits

### For Developers
- ✅ Clear structure
- ✅ Easy to navigate
- ✅ Self-documenting
- ✅ Type-safe access patterns

### For Runtime
- ✅ Clear hints for behavior
- ✅ Easy to extend with new providers
- ✅ Provider-agnostic core logic
- ✅ Metadata-driven decisions

### For Users
- ✅ Clean, readable capability files
- ✅ Easy to understand provider-specific config
- ✅ Clear discovery provenance
- ✅ Consistent pattern across all capabilities

## References
- [MCP Generic Auth Design](./MCP_GENERIC_AUTH_DESIGN.md)
- [MCP Session Management Solution](./MCP_SESSION_MANAGEMENT_SOLUTION.md)
- [Capability Directory Structure](./CAPABILITY_DIRECTORY_STRUCTURE.md)

