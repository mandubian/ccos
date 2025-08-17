# Delegation Analysis Examples

## Example 1: Simple Task - No Delegation Needed

Intent: (intent "echo-message" :goal "Echo a simple message" :constraints {} :preferences {} :success-criteria (fn [result] (string? result)))

Available Agents:
- sentiment_agent: Sentiment Analysis Agent (trust: 0.90, cost: 0.10)
- backup_agent: Backup Agent (trust: 0.80, cost: 0.20)

Response:
```json
{
  "should_delegate": false,
  "reasoning": "Simple echo task does not require specialized capabilities and can be handled directly",
  "required_capabilities": ["echo"],
  "delegation_confidence": 0.95
}
```

## Example 2: Complex Analysis - Delegation Recommended

Intent: (intent "analyze-sentiment" :goal "Analyze sentiment from user interactions" :constraints {:input-type :text} :preferences {:accuracy 0.9} :success-criteria (fn [result] (and (map? result) (contains? result :sentiment))))

Available Agents:
- sentiment_agent: Sentiment Analysis Agent (trust: 0.90, cost: 0.10)
- backup_agent: Backup Agent (trust: 0.80, cost: 0.20)

Response:
```json
{
  "should_delegate": true,
  "reasoning": "Sentiment analysis requires specialized NLP capabilities that are available in the sentiment_agent",
  "required_capabilities": ["sentiment_analysis", "text_processing"],
  "delegation_confidence": 0.92
}
```

## Example 3: Security-Sensitive Task - Delegation with High Trust

Intent: (intent "secure-backup" :goal "Create encrypted backup of sensitive data" :constraints {:encryption :required :security :high} :preferences {:reliability 0.99} :success-criteria (fn [result] (and (map? result) (contains? result :backup-id))))

Available Agents:
- sentiment_agent: Sentiment Analysis Agent (trust: 0.90, cost: 0.10)
- backup_agent: Backup Agent (trust: 0.80, cost: 0.20)

Response:
```json
{
  "should_delegate": true,
  "reasoning": "Secure backup requires specialized encryption and backup capabilities available in backup_agent",
  "required_capabilities": ["backup", "encryption"],
  "delegation_confidence": 0.88
}
```

## Example 4: No Suitable Agents - No Delegation

Intent: (intent "custom-analysis" :goal "Perform custom data analysis with specific algorithms" :constraints {:algorithm :custom} :preferences {} :success-criteria (fn [result] (map? result)))

Available Agents:
- sentiment_agent: Sentiment Analysis Agent (trust: 0.90, cost: 0.10)
- backup_agent: Backup Agent (trust: 0.80, cost: 0.20)

Response:
```json
{
  "should_delegate": false,
  "reasoning": "Custom analysis requires specific algorithms not available in current agent registry",
  "required_capabilities": ["custom_analysis"],
  "delegation_confidence": 0.85
}
```
