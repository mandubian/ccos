# Intent JSON Grammar

## Required Fields

- **name**: A descriptive identifier for the intent (snake_case)
- **goal**: Clear description of what should be achieved
- **success_criteria**: How to determine if the intent was successful

## Optional Fields

- **constraints**: Key-value pairs defining hard limits or requirements
- **preferences**: Key-value pairs defining soft preferences or priorities

## Type Rules

All constraint and preference values MUST be strings:
- ✅ Correct: `"max_cost": "100"`
- ❌ Wrong: `"max_cost": 100`

- ✅ Correct: `"priority": "high"`
- ❌ Wrong: `"priority": ["high"]`

- ✅ Correct: `"timeout": "30_seconds"`
- ❌ Wrong: `"timeout": 30`
