# Grounded Decomposition Strategy

## Tool Selection Rules

1. Examine the available tools above - each has a name, description, and input_schema.
2. For each step, if a tool matches, set "tool" to the exact tool name.
3. Extract parameters from the goal that match the tool's input_schema.
4. **If the goal does NOT provide the actual value for a parameter, create a user_input step to ask for it.** Do not invent content - ask the user!
5. If no tool matches exactly, use intent_type "api_call" or "data_transform" without a tool.
   - IMPORTANT: Do NOT force a tool match if the tool's description doesn't fit the goal.
   - It is BETTER to leave "tool" as null than to pick a wrong tool.
   - If you need a capability that isn't in the list (e.g., "group_by", "summarize", "aggregate"), use "tool": null.
6. **NO DUPLICATE TOOLS**: Each tool should typically appear ONCE in the plan.
   - API tools (like list_issues, search, get_*) ONLY fetch data - they do NOT transform it.
   - If the goal requires fetching then transforming (e.g., "fetch issues then group by label"), use TWO steps:
     a) Step 1: API call with the fetch tool (list_issues)
     b) Step 2: data_transform with "tool": null for the transformation (grouping, aggregation, etc.)
7. When data is already available, produce an "output" step (e.g., with ccos.io.println) instead of asking the user.
8. Use CONCRETE values, not placeholders. For dates, use ISO 8601 format (YYYY-MM-DD). Today is {today}.
9. For "weekly" or "last 7 days", calculate the actual date: {week_ago}.

## Intent Types

- **user_input**: Ask the user for missing information
- **api_call**: External API operation - use tool name if available
- **data_transform**: Process/filter/sort data locally
- **output**: Display results to user

## Important Output Behaviors

- ccos.data.select with count=1 returns the ITEM DIRECTLY (not a list), so use (get step_N :field) not (nth step_N 0)
- ccos.data.sort/filter/select automatically extract lists from nested map structures - no separate extraction step needed
