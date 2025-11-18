# Strategy for Tool Selection

1. **Exact Match First**: Look for tool names that exactly match keywords in the hint
2. **Semantic Matching**: If no exact match, find tools whose purpose aligns with the hint's intent
3. **Parameter Extraction**: Identify key-value pairs in the hint (e.g., "repository X", "owner Y")
4. **Tool Name Validation**: Ensure the selected tool name exists in the provided list
5. **Constraint Mapping**: Map extracted parameters to tool parameter names based on common patterns

## Common Parameter Patterns:
- "repository X" or "repo X" → `"repository": "X"`
- "owner X" or "user X" → `"owner": "X"` or `"user": "X"`
- "limit N" or "count N" → `"limit": "N"` or `"perPage": "N"`
- "state X" → `"state": "X"` (e.g., "open", "closed")

