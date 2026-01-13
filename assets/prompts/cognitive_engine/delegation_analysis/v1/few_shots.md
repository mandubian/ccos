# Few-Shot Examples

Input Intent: analyze sentiment of product reviews
Output:
{"should_delegate": true, "reasoning": "NLP classification benefits from specialized sentiment agent.", "required_capabilities": ["nlp.sentiment"], "delegation_confidence": 0.82}

Input Intent: echo a short message
Output:
{"should_delegate": false, "reasoning": "Simple local capability sufficient.", "required_capabilities": [], "delegation_confidence": 0.14}
