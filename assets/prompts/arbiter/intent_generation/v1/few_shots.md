# Intent Generation Examples

## Example 1: Simple Task

**Input**: "Echo hello world"

**Output**:
```json
{
  "name": "echo_greeting",
  "goal": "Display a greeting message to the user",
  "constraints": {},
  "preferences": {},
  "success_criteria": "Message 'hello world' is displayed"
}
```

## Example 2: With Constraints

**Input**: "Calculate the sum of 5 and 3, must be accurate"

**Output**:
```json
{
  "name": "calculate_sum",
  "goal": "Compute the mathematical sum of two numbers",
  "constraints": {
    "accuracy": "exact",
    "operand1": "5",
    "operand2": "3"
  },
  "preferences": {},
  "success_criteria": "Result equals 8"
}
```

## Example 3: With Preferences

**Input**: "Generate a report, preferably in PDF format, keep it under 10 pages"

**Output**:
```json
{
  "name": "generate_report",
  "goal": "Create a comprehensive report document",
  "constraints": {
    "max_pages": "10"
  },
  "preferences": {
    "format": "pdf",
    "style": "professional"
  },
  "success_criteria": "Report is generated with all required sections and under page limit"
}
```
