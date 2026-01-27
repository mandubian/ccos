---
name: Capability Development
description: Guide for creating, registering, and managing CCOS/RTFS capabilities
---

# Capability Development Guide

Capabilities are the core building blocks of CCOS - typed, auditable units of functionality.

## Capability File Structure

Capabilities are defined in `.rtfs` files using S-expression syntax:

```clojure
(capability "domain.capability-name"
  :description "What this capability does"
  :input-schema [:map
    [:param1 :string]
    [:param2 [:and :int [:> 0]]]]
  :output-schema [:map [:result :any]]
  :effects [:network]                    ; Declared effects
  :implementation (fn [inputs]
    ;; Pure logic + host calls
    (let [url (str "https://api.example.com/" (:param1 inputs))]
      (call "ccos.network.http-fetch" {:url url}))))
```

## Schema Definitions

### Input Schema
Use RTFS type expressions for validation:

```clojure
:input-schema [:map
  [:required_param :string]                      ; Required string
  [:optional_param {:optional true} :int]        ; Optional int
  [:bounded [:and :int [:>= 0] [:< 100]]]       ; Constrained int
  [:enum_field [:union "a" "b" "c"]]            ; Enum values
  [:nested [:map [:inner :string]]]]            ; Nested map
```

### Output Schema
```clojure
:output-schema [:map
  [:data [:vector :any]]
  [:count :int]
  [:metadata {:optional true} :any]]
```

## Server Manifests

For external API integrations, create a `server.rtfs` file:

```clojure
(server
  :id "weather_api"
  :server_info {
    "name" "Weather API"
    "endpoint" "https://api.weather.com"
    "description" "Weather data service"
    "auth_env_var" "WEATHER_API_KEY"
  }
  :capability_files ["openapi/v1/get_forecast.rtfs" 
                     "openapi/v1/get_current.rtfs"]
  :source {"type" "OpenAPI" 
           "entry" {"url" "https://api.weather.com/openapi.json"}}
)
```

## Introspecting External APIs

Use MCP tools to discover and register external APIs:

### Step 1: Suggest APIs
```
ccos_suggest_apis { query: "weather forecast API" }
```

### Step 2: Introspect
```
ccos_introspect_remote_api { 
  endpoint: "https://api.openweathermap.org/data/3.0/openapi.json",
  name: "OpenWeatherMap",
  auth_env_var: "OPENWEATHERMAP_API_KEY"
}
```

### Step 3: Approve & Register
```
# User approves via /approvals UI, then:
ccos_register_server { approval_id: "..." }
```

## Agent Capabilities

For multi-step workflows, use the `:agent` kind:

```clojure
(capability "agent.daily-reporter"
  :description "Generates consolidated daily report"
  :input-schema [:map
    [:city :string?]
    [:btc_symbol :string?]]
  :output-schema [:map
    [:weather :any]
    [:btc :any]]
  :meta {
    :kind :agent
    :planning false
    :source-session "session_123"
  }
  :implementation (fn [inputs]
    (let [city (:city inputs "Paris")
          weather (call "weather.get_current" {:city city})
          btc (call "crypto.get_price" {:symbol (:btc_symbol inputs "BTC")})]
      {:weather weather :btc btc})))
```

## Synthesis via MCP

Generate capabilities programmatically:

```
ccos_synthesize_capability {
  description: "Fetch and format cryptocurrency prices",
  capability_name: "crypto.formatted_prices",
  input_schema: {
    type: "object",
    properties: {
      symbols: { type: "array", items: { type: "string" } }
    }
  }
}
```

## Session → Capability Flow

Convert interactive sessions into reusable agents:

```
1. ccos_session_start { goal: "Build weather+crypto reporter" }
2. ccos_execute_capability { ... }  # Execute steps
3. ccos_execute_capability { ... }
4. ccos_session_end { session_id: "...", save_as: "workflow.rtfs" }
5. ccos_consolidate_session { 
     session_id: "...", 
     agent_name: "daily_reporter" 
   }
```

## Directory Structure

```
capabilities/
├── core/                    # Built-in capabilities
├── servers/
│   ├── approved/           # Approved external APIs
│   │   └── weather_api/
│   │       ├── server.rtfs
│   │       └── openapi/v1/
│   │           └── get_forecast.rtfs
│   └── pending/            # Pending approval
├── agents/                  # Agent capabilities
│   └── daily-reporter.rtfs
└── learned/                 # Auto-synthesized
```

## Effect Categories

Declare effects for governance:

| Effect | Description |
|--------|-------------|
| `:network` | HTTP requests, API calls |
| `:io` | Console, logging |
| `:fs` | File system access |
| `:state` | Key-value store, database |
| `:time` | Current time access |
| `:random` | Random number generation |

## Best Practices

1. **Use specific IDs**: `domain.action` format (e.g., `github.list_issues`)
2. **Validate inputs**: Use RTFS type constraints for safety
3. **Declare effects**: Enables governance auditing
4. **Keep implementations pure**: Delegate all effects via `(call ...)`
5. **Document well**: Clear descriptions help agent discovery
