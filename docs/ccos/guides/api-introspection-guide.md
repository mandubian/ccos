# API Introspection for Multi-Capability Synthesis

## Overview

This module implements automatic API discovery and multi-capability synthesis from API specifications. It introspects APIs (OpenAPI/Swagger) and generates multiple specialized RTFS capabilities—one per endpoint—with proper schema encoding and runtime-controlled implementations.

## Key Components

### 1. `api_introspector.rs`
- **Purpose:** Discover API endpoints and convert schemas to RTFS types
- **Features:**
  - Parse OpenAPI specifications
  - Convert JSON Schema → RTFS `TypeExpr`
  - Generate `CapabilityManifest` per endpoint
  - Serialize capabilities to `.rtfs` format

### 2. `capability_synthesizer.rs`
- **New Method:** `synthesize_from_api_introspection(api_url, api_domain)`
- **Purpose:** Orchestrate introspection and capability generation
- **Output:** `MultiCapabilitySynthesisResult` with multiple capabilities

### 3. Test Binaries

#### `test_multi_capability_synthesis`
Demonstrates API introspection vs legacy hardcoded approach

#### `test_openweather_introspection`
Generates OpenWeather API capabilities with proper schemas

#### `call_introspected_openweather`
Tests calling the generated capabilities with real HTTP requests

## Usage Example

### Generate Capabilities from API
```rust
let synthesizer = CapabilitySynthesizer::mock();
let result = synthesizer
    .synthesize_from_api_introspection("https://api.example.com", "example")
    .await?;

for cap in result.capabilities {
    println!("Generated: {}", cap.capability.id);
}
```

### Generated Capability File
```clojure
(capability "openweather_api.get_current_weather"
  :name "Get Current Weather"
  :version "2.5"
  :input-schema {
    :q :string ;; optional
    :lat :float ;; optional
    :units :string ;; optional
  }
  :output-schema {
    :coord { :lon :float :lat :float }
    :main { :temp :float :humidity :int }
    :name :string
  }
  :implementation
    (fn [input]
      ;; Runtime-controlled: validation, auth, rate limiting
      (let [final_url (build-url-with-params input)
            api_key (call "ccos.system.get-env" "API_KEY")]
        (call "ccos.network.http-fetch" :url final_url ...))))
```

### Call the Capability
```clojure
;; Type-safe, validated by runtime
((call "openweather_api.get_current_weather") {
  :q "London,UK"
  :units "metric"
})
```

## Key Improvements

### Schema Encoding
**Before:** `:input-schema :any` (no type safety)  
**After:** Fully typed with `:string`, `:int`, `:float`, nested maps, vectors

### Runtime Controls
**Before:** Validation/auth hardcoded in implementation (~90 lines)  
**After:** Delegated to runtime (~30 lines, clean)

### Multi-Capability
**Before:** 1 generic wrapper for all endpoints  
**After:** 1 specialized capability per endpoint

## Architecture

```
API Spec (OpenAPI)
        ↓
   [Introspection]
        ↓
   Endpoints Discovery
        ↓
   Schema Conversion (JSON → RTFS TypeExpr)
        ↓
   Capability Generation (1 per endpoint)
        ↓
   RTFS Serialization
        ↓
   capability.rtfs files
```

## Testing

```bash
# Generate capabilities
cd rtfs_compiler
cargo run --bin test_openweather_introspection

# Test calling them (set your API key first)
export OPENWEATHERMAP_ORG_API_KEY='your_key'
cargo run --bin call_introspected_openweather
```

## Benefits

✅ **Automatic Discovery** - No manual endpoint specification  
✅ **Type Safety** - Full RTFS schemas for validation  
✅ **Runtime Controls** - Validation, auth, rate limiting by runtime  
✅ **Clean Code** - Simple implementations, no control logic  
✅ **Multi-Capability** - Specialized per endpoint  
✅ **Maintainable** - Auto-regenerate from updated specs  

## Files Modified

- `api_introspector.rs` (NEW) - 1,276 lines
- `capability_synthesizer.rs` - Added introspection method
- `test_multi_capability_synthesis.rs` - Comparison demo
- `test_openweather_introspection.rs` (NEW) - OpenWeather demo
- `call_introspected_openweather.rs` (NEW) - Live test binary
- `mod.rs` - Exported new module

