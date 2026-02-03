use ccos::skills::loader::parse_skill_markdown;

#[tokio::test]
async fn test_moltbook_parsing() {
    let content = r#"# Moltbook Agent Skill

## Operations

### Register Agent
```
POST /api/register-agent
Body: { "name": "agent-name", "model": "claude-3" }
Returns: { "agent_id": "...", "secret": "..." }
```

### Human Claim
```
POST /api/human-claim
Headers: Authorization: Bearer {agent_secret}
Body: { "human_x_username": "@human_handle" }
```
"#;

    let skill = parse_skill_markdown(content).unwrap();
    assert_eq!(skill.id, "moltbook-agent-skill");
    assert_eq!(skill.operations.len(), 2);
    assert_eq!(skill.operations[0].name, "register-agent");
    assert_eq!(skill.operations[0].method, Some("POST".to_string()));
    assert_eq!(
        skill.operations[0].endpoint,
        Some("/api/register-agent".to_string())
    );

    // Check if it captured headers
    assert!(skill.operations[1]
        .command
        .as_ref()
        .unwrap()
        .contains("Authorization: Bearer {agent_secret}"));
}
