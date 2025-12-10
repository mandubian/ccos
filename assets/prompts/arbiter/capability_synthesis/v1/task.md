Goal: synthesize a PURE RTFS capability.

Capability ID: {capability_id}
Description: {description}
Intent Type: {intent_type}
Context JSON: {context_json}

Instructions:
- Produce ONLY the RTFS capability form, no explanations.
- Use the grammar and strategy above.
- Infer precise input/output schemas from the `Context JSON` (grounding data). Do NOT default to `:any`.
- Create a realistic `:sample-input` and `:sample-output` in metadata based on the `Context JSON`. These MUST be RTFS-formatted strings (e.g. `"{:count 1}"`).
- Prefer to read input from `:data` (vector of maps) unless context suggests otherwise.
- Guard against nil/missing fields; return empty map/vector when no data.
- For grouping/counting: output a map label -> {:count n :last_updated "<ts>"}.
- Ensure :effects [:pure].
- Ensure :input-schema and :output-schema are precise and valid.

Output:
```
(capability ...)
```








