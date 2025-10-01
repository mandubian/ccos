# Delegation Analysis Task
Determine whether the current intent should be delegated to a specialized agent.
Respond ONLY with a JSON object containing:
{
  "should_delegate": true|false,
  "reasoning": "short explanation",
  "required_capabilities": ["cap1", "cap2"],
  "delegation_confidence": 0.0-1.0
}
Rules:
- All four fields required.
- reasoning concise (< 240 chars).
- delegation_confidence is a float 0.0-1.0.
- required_capabilities empty array if none.
