# Intent Generation Anti-Patterns

## ❌ Non-String Constraint Values

**Wrong**:
```json
{
  "constraints": {
    "max_cost": 100
  }
}
```

**Correct**:
```json
{
  "constraints": {
    "max_cost": "100"
  }
}
```

## ❌ Array Values in Preferences

**Wrong**:
```json
{
  "preferences": {
    "priority": ["high", "urgent"]
  }
}
```

**Correct**:
```json
{
  "preferences": {
    "priority": "high",
    "urgency": "urgent"
  }
}
```

## ❌ Vague Success Criteria

**Wrong**:
```json
{
  "success_criteria": "It works"
}
```

**Correct**:
```json
{
  "success_criteria": "All data is processed without errors and results are saved to database"
}
```

## ❌ Missing Required Fields

**Wrong**:
```json
{
  "name": "some_task"
}
```

**Correct**:
```json
{
  "name": "some_task",
  "goal": "Accomplish something specific",
  "success_criteria": "Task completes successfully"
}
```
