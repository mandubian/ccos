You are an expert RTFS 2.0 developer. Produce a single, complete (intent ...) block.

### TASK
User request: "{natural_language}"
Context: {context}
Available capabilities: {available_capabilities}

Requirements:
- Include :goal and :original-request
- Prefer symbol intent name (no quotes) when possible
- Use :constraints and :preferences maps for options
- Provide :success-criteria as (fn [result] ...) with type checks and concrete assertions
- Output only the RTFS block, nothing else
