# Delegation Analysis Anti-Patterns

## ANTI-PATTERN 1: Invalid JSON Format
INCORRECT:
```
I think we should delegate this task because it's complex.
{
  should_delegate: true,
  reasoning: "Complex task"
}
```

CORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Complex task requires specialized capabilities"
}
```

## ANTI-PATTERN 2: Missing Required Fields
INCORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Task is complex"
}
```

CORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Task is complex",
  "required_capabilities": ["specialized_capability"],
  "delegation_confidence": 0.85
}
```

## ANTI-PATTERN 3: Wrong Data Types
INCORRECT:
```json
{
  "should_delegate": "yes",
  "reasoning": "Complex task",
  "required_capabilities": "capability1",
  "delegation_confidence": "0.9"
}
```

CORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Complex task",
  "required_capabilities": ["capability1"],
  "delegation_confidence": 0.9
}
```

## ANTI-PATTERN 4: Extra Text Outside JSON
INCORRECT:
```
Let me analyze this task...

{
  "should_delegate": true,
  "reasoning": "Complex task",
  "required_capabilities": ["capability1"],
  "delegation_confidence": 0.9
}

This is my analysis.
```

CORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Complex task",
  "required_capabilities": ["capability1"],
  "delegation_confidence": 0.9
}
```

## ANTI-PATTERN 5: Inconsistent Confidence Values
INCORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Task is complex",
  "required_capabilities": ["capability1"],
  "delegation_confidence": 1.5
}
```

CORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Task is complex",
  "required_capabilities": ["capability1"],
  "delegation_confidence": 0.9
}
```

## ANTI-PATTERN 6: Empty or Vague Reasoning
INCORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Yes",
  "required_capabilities": ["capability1"],
  "delegation_confidence": 0.9
}
```

CORRECT:
```json
{
  "should_delegate": true,
  "reasoning": "Task requires specialized sentiment analysis capabilities available in the agent registry",
  "required_capabilities": ["sentiment_analysis"],
  "delegation_confidence": 0.9
}
```
