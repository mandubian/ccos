# Capability Directory Structure

## 📁 Overview

CCOS organizes generated capabilities in a hierarchical directory structure that reflects the capability's provider type, namespace, and identity.

## 🗂️ Directory Layout

```
capabilities/
├── mcp/
│   ├── github/
│   │   ├── list_issues.rtfs
│   │   ├── create_issue.rtfs
│   │   ├── create_pull_request.rtfs
│   │   ├── search_code.rtfs
│   │   └── ... (46 tools total)
│   ├── slack/
│   │   ├── send_message.rtfs
│   │   └── ...
│   └── jira/
│       ├── create_ticket.rtfs
│       └── ...
└── openapi/
    ├── openweather/
    │   ├── get_current_weather.rtfs
    │   └── get_forecast.rtfs
    ├── stripe/
    │   ├── create_payment.rtfs
    │   └── ...
    └── twilio/
        ├── send_sms.rtfs
        └── ...
```

## 📊 Structure Breakdown

### Level 1: Provider Type

The first level indicates how the capability is provided:

- **`mcp/`** - Capabilities from MCP (Model Context Protocol) servers
- **`openapi/`** - Capabilities from OpenAPI/REST APIs
- **`local/`** - Local RTFS capabilities (future)
- **`grpc/`** - gRPC service capabilities (future)

### Level 2: Namespace

The second level groups capabilities by their service/API namespace:

- **MCP**: Server name (e.g., `github`, `slack`, `jira`)
- **OpenAPI**: API name (e.g., `openweather`, `stripe`, `twilio`)

### Level 3: Capability File

Individual capability as RTFS file:

- File name: `<tool_or_endpoint_name>.rtfs`
- Contains complete capability definition
- Directly loadable by CCOS runtime

## 🎯 Benefits

### 1. Clear Organization

```
✅ capabilities/mcp/github/list_issues.rtfs
❌ capabilities/mcp.github.list_issues/capability.rtfs  (old)
```

- Obvious provider type at first glance
- Natural grouping by namespace
- No redundant directory nesting

### 2. Easy Discovery

```bash
# Find all GitHub capabilities
ls capabilities/mcp/github/

# Find all OpenAPI capabilities
ls capabilities/openapi/*/

# Count MCP capabilities
find capabilities/mcp -name "*.rtfs" | wc -l
```

### 3. Direct Access

```clojure
;; Load a specific capability
(load-capability "capabilities/mcp/github/list_issues.rtfs")

;; Load all GitHub capabilities
(load-capabilities-from-dir "capabilities/mcp/github/")

;; Load all MCP capabilities
(load-capabilities-from-dir "capabilities/mcp/")
```

### 4. Namespace Isolation

Different namespaces can have identically named tools without conflict:

```
capabilities/mcp/github/list.rtfs      ← GitHub's list
capabilities/mcp/jira/list.rtfs        ← Jira's list
capabilities/openapi/todoist/list.rtfs ← Todoist's list
```

### 5. Scalability

Easy to add new providers and namespaces:

```bash
# Add new MCP server
mkdir capabilities/mcp/notion
# Generate capabilities...

# Add new OpenAPI service
mkdir capabilities/openapi/github
# Generate capabilities...
```

## 🔧 Implementation Details

### MCP Capabilities

**Capability ID Format**: `mcp.<namespace>.<tool_name>`

**Directory Mapping**:
```
ID: mcp.github.list_issues
    │    │        └─ Tool name
    │    └─ Namespace
    └─ Provider type

Path: capabilities/mcp/github/list_issues.rtfs
      │            │   │      └─ tool_name.rtfs
      │            │   └─ namespace/
      │            └─ mcp/
      └─ capabilities/
```

**Multi-word Tool Names**:
```
ID:   mcp.github.add_comment_to_pending_review
Path: capabilities/mcp/github/add_comment_to_pending_review.rtfs
```

### OpenAPI Capabilities

**Capability ID Format**: `<api_name>_api.<endpoint_name>`

**Directory Mapping**:
```
ID: openweather_api.get_current_weather
    │              └─ Endpoint name
    └─ API name (with _api suffix)

Path: capabilities/openapi/openweather/get_current_weather.rtfs
      │            │       │          └─ endpoint.rtfs
      │            │       └─ api_name/ (suffix removed)
      │            └─ openapi/
      └─ capabilities/
```

## 📝 File Naming Conventions

### Tool/Endpoint Names

- **Use underscores** for multi-word names: `list_issues.rtfs`
- **Lowercase** for consistency: `create_pull_request.rtfs`
- **Descriptive** names matching the actual function: `get_current_weather.rtfs`

### Namespace Names

- **Lowercase** service names: `github`, `slack`, `openweather`
- **No special characters**: Use hyphens if needed: `my-service`
- **Short but clear**: `gh` → `github` ✅

## 🚀 Usage Examples

### Loading Capabilities in CCOS Plans

```clojure
;; Load specific capability
(do
  (load-capability "capabilities/mcp/github/list_issues.rtfs")
  
  ;; Use it
  ((call "mcp.github.list_issues") {
    :owner "mandubian"
    :repo "ccos"
    :state "open"
  }))
```

### Discovering Available Capabilities

```clojure
;; List all GitHub MCP tools
(list-capabilities-in "capabilities/mcp/github/")
;; Returns: [list_issues, create_issue, create_pull_request, ...]

;; List all OpenAPI services
(list-namespaces-in "capabilities/openapi/")
;; Returns: [openweather, stripe, twilio, ...]
```

### Batch Loading

```clojure
;; Load all GitHub capabilities at once
(load-all-capabilities-from "capabilities/mcp/github/")

;; Load all MCP capabilities
(load-all-capabilities-from "capabilities/mcp/")
```

## 📊 Current Statistics

As of this implementation:

- **MCP Capabilities**: 46 GitHub tools
- **OpenAPI Capabilities**: 2 OpenWeather endpoints
- **Total Namespaces**: 2 (github, openweather)
- **Provider Types**: 2 (mcp, openapi)

## 🔮 Future Extensions

### Planned Provider Types

```
capabilities/
├── mcp/          ← MCP servers
├── openapi/      ← REST APIs
├── grpc/         ← gRPC services
├── graphql/      ← GraphQL APIs
├── local/        ← Local RTFS functions
└── hybrid/       ← Combined capabilities
```

### Metadata Index (Future)

```
capabilities/
├── index.json    ← Capability registry
├── mcp/
│   └── github/
│       ├── .metadata.json  ← Namespace metadata
│       └── list_issues.rtfs
└── openapi/
    └── openweather/
        ├── .metadata.json
        └── get_current_weather.rtfs
```

## ✅ Migration Notes

### Old Structure (Deprecated)

```
capabilities/
├── mcp.github.list_issues/
│   └── capability.rtfs
└── openweather_api.get_current_weather/
    └── capability.rtfs
```

### New Structure (Current)

```
capabilities/
├── mcp/github/list_issues.rtfs
└── openapi/openweather/get_current_weather.rtfs
```

**Migration**: Automatic - regenerate capabilities using updated introspectors

## 🎉 Conclusion

The hierarchical capability directory structure provides:
- ✅ Clear organization by provider and namespace
- ✅ Easy discovery and navigation
- ✅ Direct file access without nested directories
- ✅ Namespace isolation preventing conflicts
- ✅ Scalable architecture for many services

This structure makes CCOS capabilities **easy to discover, understand, and use**! 🚀

