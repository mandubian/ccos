//! Working Memory Integration Tests
//!
//! These tests demonstrate the full end-to-end Working Memory implementation
//! with real-time event-driven synchronization between Causal Chain and Working Memory.

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::types::{Action, ActionType, ExecutionResult};
use rtfs_compiler::ccos::wm_integration::WmIngestionSink;
use rtfs_compiler::ccos::working_memory::backend::QueryParams;
use rtfs_compiler::ccos::working_memory::backend_inmemory::InMemoryJsonlBackend;
use rtfs_compiler::ccos::working_memory::facade::WorkingMemory;
use rtfs_compiler::ccos::event_sink::CausalChainEventSink;
use rtfs_compiler::runtime::values::Value;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() * 1000 // Convert to milliseconds
}

fn create_map_from_json(json_val: serde_json::Value) -> HashMap<MapKey, Value> {
    let mut map = HashMap::new();
    if let Some(obj) = json_val.as_object() {
        for (k, v) in obj {
            let key = MapKey::String(k.clone());
            let value = Value::String(v.to_string());
            map.insert(key, value);
        }
    }
    map
}

#[tokio::test]
async fn test_end_to_end_working_memory_integration() {
    // Create Causal Chain
    let mut chain = CausalChain::new().unwrap();
    
    // Create Working Memory with bridge
    let backend = InMemoryJsonlBackend::new(None, Some(100), Some(10000));
    let working_memory = Arc::new(Mutex::new(WorkingMemory::new(Box::new(backend))));
    let wm_sink = WmIngestionSink::new(working_memory.clone());
    
    // Register the integration sink
    chain.register_event_sink(Arc::new(wm_sink));
    
    let now = now_timestamp();
    
    // Test action 1: Plan execution start
    let mut metadata1 = HashMap::new();
    metadata1.insert("agent_id".to_string(), Value::String("test-agent".to_string()));
    metadata1.insert("content".to_string(), Value::Map(create_map_from_json(json!({"plan": "github-analysis", "status": "started"}))));
    
    let action1 = Action {
        action_id: "action-001".to_string(),
        parent_action_id: None,
        plan_id: "plan-001".to_string(),
        intent_id: "intent-001".to_string(),
        action_type: ActionType::PlanStarted,
        function_name: Some("github-analysis".to_string()),
        arguments: Some(vec![Value::Map(create_map_from_json(json!({"repo": "mandubian/ccos", "task": "analyze"})))]),
        result: None,
        cost: Some(0.1),
        duration_ms: Some(500),
        timestamp: now - 10,
        metadata: metadata1,
    };

    // Append action - should trigger Working Memory ingestion
    chain.append(&action1).unwrap();
    
    println!("After appending action1, checking Working Memory...");
    
    // Give some time for async processing if any
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Test action 2: Plan execution step 
    let mut metadata2 = HashMap::new();
    metadata2.insert("agent_id".to_string(), Value::String("test-agent".to_string()));
    metadata2.insert("content".to_string(), Value::Map(create_map_from_json(json!({"plan": "analyze-repositories", "step": 1}))));
    
    let action2 = Action {
        action_id: "action-002".to_string(),
        parent_action_id: Some("action-001".to_string()),
        plan_id: "plan-002".to_string(),
        intent_id: "intent-001".to_string(),
        action_type: ActionType::PlanStepCompleted,
        function_name: Some("analyze-step".to_string()),
        arguments: Some(vec![Value::Map(create_map_from_json(json!({"step": 1})))]),
        result: Some(ExecutionResult {
            success: true,
            value: Value::String("analysis complete".to_string()),
            metadata: HashMap::new(),
        }),
        cost: Some(0.05),
        duration_ms: Some(300),
        timestamp: now - 5,
        metadata: metadata2,
    };

    chain.append(&action2).unwrap();
    
    println!("After appending action2, checking Working Memory...");
    
    // Give some time for async processing if any
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Verify Working Memory has been populated automatically
    let wm = working_memory.lock().unwrap();
    
    println!("Checking Working Memory contents...");
    
    // Query all entries since we don't expect agent tags in our simple conversion
    let all_query = QueryParams::default().with_limit(Some(10));
    let all_actions = wm.query(&all_query).unwrap();
    
    println!("Found {} entries in Working Memory", all_actions.entries.len());
    for (i, entry) in all_actions.entries.iter().enumerate() {
        println!("Entry {}: ID={}, title={}, tags={:?}", i, entry.id, entry.title, entry.tags);
    }
    
    assert_eq!(all_actions.entries.len(), 2, "Expected 2 actions to be ingested");
    
    // Check that entries contain expected action IDs in their working memory IDs
    let has_action_001 = all_actions.entries.iter().any(|e| e.id.contains("action-001"));
    let has_action_002 = all_actions.entries.iter().any(|e| e.id.contains("action-002"));
    assert!(has_action_001, "Should contain action-001");
    assert!(has_action_002, "Should contain action-002");
    
    // Query by time range  
    let time_query = QueryParams::default()
        .with_time_window(Some((now - 3600000) / 1000), Some(now / 1000))  // Convert to seconds
        .with_limit(Some(10));
    let time_actions = wm.query(&time_query).unwrap();
    assert_eq!(time_actions.entries.len(), 2);
    
    // Check that entries have expected tags
    let first_entry = &all_actions.entries[0];
    assert!(first_entry.tags.contains(&"causal-chain".to_string()));
    assert!(first_entry.tags.contains(&"distillation".to_string()));
    assert!(first_entry.tags.contains(&"wisdom".to_string()));

    println!("✅ End-to-end Working Memory integration working correctly");
}

#[tokio::test]
async fn test_working_memory_query_apis_for_arbiter_context_horizon() {
    // This test demonstrates the query APIs needed for Arbiter and Context Horizon
    
    // Rebuild from causal chain
    let mut chain = CausalChain::new().unwrap();
    let backend = InMemoryJsonlBackend::new(None, Some(1000), Some(100000));
    let working_memory = Arc::new(Mutex::new(WorkingMemory::new(Box::new(backend))));
    
    let wm_sink = WmIngestionSink::new(working_memory.clone());
    chain.register_event_sink(Arc::new(wm_sink));
    
    let now = now_timestamp();
    
    // Create diverse test actions
    let agents = ["agent-alice", "agent-bob", "agent-charlie"];
    let action_types = [ActionType::CapabilityCall, ActionType::PlanStepStarted, ActionType::PlanStepCompleted];
    let contents = [
        json!({"activity": "data-mining", "source": "github"}),
        json!({"activity": "capability-discovery", "provider": "openai"}),
        json!({"activity": "plan-optimization", "target": "efficiency"}),
    ];

    // Populate with test data
    for i in 0..15 {
        let agent = agents[i % agents.len()];
        let action_type = action_types[i % action_types.len()].clone();
        let content = &contents[i % contents.len()];
        
        let mut metadata = HashMap::new();
        metadata.insert("agent_id".to_string(), Value::String(agent.to_string()));
        metadata.insert("content".to_string(), Value::Map(create_map_from_json(content.clone())));
        
        let action = Action {
            action_id: format!("action-{:03}", i + 1),
            parent_action_id: if i > 0 { Some(format!("action-{:03}", i)) } else { None },
            plan_id: format!("plan-{}", i % 3 + 1),
            intent_id: format!("intent-{}", i % 3 + 1),
            action_type,
            function_name: Some(format!("function-{}", i + 1)),
            arguments: Some(vec![Value::Map(create_map_from_json(content.clone()))]),
            result: Some(ExecutionResult {
                success: true,
                value: Value::String("completed".to_string()),
                metadata: HashMap::new(),
            }),
            cost: Some(0.01),
            duration_ms: Some(100),
            timestamp: now - (30 * 60 * 1000) + ((i as u64) * 2 * 60 * 1000), // Spread every 2 minutes over 30 min
            metadata,
        };
        
        chain.append(&action).unwrap();
    }
    
    // Now test the Working Memory query APIs used by Arbiter/Context Horizon
    let wm = working_memory.lock().unwrap();
    
    // 1. Check that we have all actions ingested
    let all_query = QueryParams::default().with_limit(Some(20));
    let all_actions = wm.query(&all_query).unwrap();
    assert!(all_actions.entries.len() >= 15, "Should have 15 actions ingested");

    // 2. Time-based queries (Context Horizon for recent activity)
    let fifteen_min_ago_s = (now - 900000) / 1000; // 15 minutes ago in seconds
    let now_s = (now + 100000) / 1000; // Now plus buffer in seconds
    println!("Time window: {} to {}", fifteen_min_ago_s, now_s);
    println!("Now in ms: {}, fifteen_min_ago in ms: {}", now, now - 900000);
    
    let recent_query = QueryParams::default()
        .with_time_window(Some(fifteen_min_ago_s), Some(now_s))
        .with_limit(Some(20));
    let recent_actions = wm.query(&recent_query).unwrap();
    
    // Debug the timestamps in Working Memory
    for (i, entry) in all_actions.entries.iter().enumerate() {
        println!("Action {}: timestamp_s={}, in range: {}", i, entry.timestamp_s, 
                 entry.timestamp_s >= fifteen_min_ago_s && entry.timestamp_s <= now_s);
    }
    
    println!("Found {} recent actions out of {} total", recent_actions.entries.len(), all_actions.entries.len());
    assert!(recent_actions.entries.len() >= 7, "Recent actions should be available");
    
    // 3. Content-based queries using action type tags
    let capability_query = QueryParams::with_tags(["capabilitycall"]).with_limit(Some(10));
    let capability_calls = wm.query(&capability_query).unwrap();
    assert!(capability_calls.entries.len() >= 2, "Should find capability call actions");

    let plan_query = QueryParams::with_tags(["planstepstarted"]).with_limit(Some(10));
    let plan_actions = wm.query(&plan_query).unwrap();
    assert!(plan_actions.entries.len() >= 2, "Should find plan step actions");

    // 4. Bounded queries (prevent memory overflow)
    let limited_query = QueryParams::default().with_limit(Some(1));
    let limited_actions = wm.query(&limited_query).unwrap();
    assert_eq!(limited_actions.entries.len(), 1);
    
    println!("✅ Working Memory query APIs ready for Arbiter and Context Horizon");
}

#[tokio::test]
async fn test_working_memory_bridge_integration() {
    // Test the WmIngestionSink as event bridge
    
    let backend = InMemoryJsonlBackend::new(None, Some(10), Some(10_000));
    let wm = Arc::new(Mutex::new(WorkingMemory::new(Box::new(backend))));
    let sink = WmIngestionSink::new(wm.clone());

    // Simulate causal chain appending an action
    let mut metadata = HashMap::new();
    metadata.insert("signature".to_string(), Value::String("test-signature".to_string()));
    
    let action = Action {
        action_id: "test-action-001".to_string(),
        parent_action_id: None,
        plan_id: "test-plan-001".to_string(),
        intent_id: "test-intent-001".to_string(),
        action_type: ActionType::PlanStarted,
        function_name: Some("test-function".to_string()),
        arguments: Some(vec![Value::String("test-arg".to_string())]),
        result: Some(ExecutionResult {
            success: true,
            value: Value::String("test-result".to_string()),
            metadata: HashMap::new(),
        }),
        cost: Some(0.1),
        duration_ms: Some(100),
        timestamp: now_timestamp(),
        metadata,
    };
    
    // Trigger the event sink
    sink.on_action_appended(&action);

    // Verify the action was ingested into working memory
    let wm_lock = wm.lock().unwrap();
    let results = wm_lock.query(&QueryParams::default()).unwrap();
    assert_eq!(results.entries.len(), 1);
    
    let entry = &results.entries[0];
    assert!(entry.id.contains("test-action-001"), "Entry ID should contain action ID");
    assert!(entry.tags.contains(&"causal-chain".to_string()));
    assert!(entry.tags.contains(&"distillation".to_string()));
    assert!(entry.tags.contains(&"wisdom".to_string()));
    assert!(entry.tags.contains(&"planstarted".to_string())); // lowercased action type
    
    println!("✅ Working Memory bridge integration working correctly");
}
