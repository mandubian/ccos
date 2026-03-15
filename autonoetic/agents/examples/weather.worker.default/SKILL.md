---
name: "weather.worker.default"
description: "Deterministic weather data retrieval agent (script-only, no LLM)."
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "weather.worker.default"
      name: "Weather Worker"
      description: "Retrieves simulated weather data for a given location."
    capabilities: []
    execution_mode: "script"
    script_entry: "scripts/get_weather.py"
io:
  accepts:
    type: object
    properties:
      location:
        type: string
        description: "City name to get weather for"
    required: [location]
  returns:
    type: object
    properties:
      location:
        type: string
      temperature_c:
        type: number
      condition:
        type: string
      humidity_percent:
        type: integer
---
# Weather Worker

A deterministic, script-only agent that retrieves simulated weather data.

## Execution Mode

This agent uses `execution_mode: script` which means:
- **No LLM** - runs directly in sandbox without invoking a language model
- **Fast** - completes in ~100ms (no LLM latency)
- **Free** - no token usage or API calls
- **Deterministic** - same input always produces same output

## Input

```json
{"location": "tokyo"}
```

## Output

```json
{
  "location": "tokyo",
  "temperature_c": 22,
  "condition": "partly cloudy",
  "humidity_percent": 65,
  "source": "simulated"
}
```

## Supported Locations

- tokyo, london, new york, paris, sydney, berlin
- Any other location returns default values

## When to Use Script Mode

Use `execution_mode: script` for:
- API calls with fixed format (weather, stocks, crypto)
- Data transformation (JSON→CSV, format conversion)
- Simple lookups (database query, cache read)
- Status checks (health check, service status)

Use reasoning mode (default) for:
- Multi-step reasoning
- Research and synthesis
- Ambiguous requirements
