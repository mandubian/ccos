# Task: Select MCP Tool from List

You are helping to select the best MCP (Model Context Protocol) tool from a provided list based on a natural language hint.

## Your Task:
1. Analyze the natural language hint: `{hint}`
2. Review the available tools: `{tools}`
3. Select the EXACT tool name that best matches the hint
4. Extract any relevant parameters from the hint that should be passed to the tool

{schemas}

## Response Format:
You must respond with ONLY a valid RTFS intent expression in this format:

```rtfs
(intent "{tool_name}"
  :goal "{description of what the tool will do}"
  :constraints {
    "parameter1" "{value1}"
    "parameter2" "{value2}"
  }
)
```

## Critical Rules:
- The `{tool_name}` in the intent name MUST be one of the exact tool names from the list above
- **CRITICAL: Parameter Names**: If schema information is provided above, you MUST use the EXACT parameter names from the tool's schema. Do NOT invent parameter names or use synonyms from the hint.
  - Example: If the hint says "repository ccos" but the schema shows parameter "repo", use `"repo": "ccos"` NOT `"repository": "ccos"`
  - Example: If the hint says "user mandubian" but the schema shows parameter "owner", use `"owner": "mandubian"` NOT `"user": "mandubian"`
- Extract parameter VALUES from the hint, but use the EXACT parameter NAMES from the schema
- All constraint values must be strings
- Return ONLY the RTFS expression, no prose or explanations

## Example:
Hint: "list issues of repository ccos and owner mandubian"
Tools: list_issues, search_issues, get_issue
Tool Parameter Schemas:
  - list_issues: parameters: repo, owner, perPage, state

Response:
```rtfs
(intent "list_issues"
  :goal "List issues for repository ccos owned by mandubian"
  :constraints {
    "repo" "ccos"
    "owner" "mandubian"
  }
)
```

Note: Even though the hint says "repository", the schema parameter is "repo", so we use "repo" in the constraints.

