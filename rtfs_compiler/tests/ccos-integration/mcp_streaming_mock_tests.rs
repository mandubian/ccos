use std::sync::Arc;
use rtfs_compiler::{
    ccos::{
        capability_marketplace::CapabilityMarketplace,
        streaming::{register_mcp_streaming_capability, mock_loop::run_mock_stream_loop, rtfs_streaming_syntax::maybe_lower_mcp_stream_macro},
    },
    ast::{Expression, Symbol, Literal, Keyword, MapKey},
    runtime::{
        streaming::{McpStreamingProvider, StreamingCapability, StreamStatus},
        values::Value,
    }
};
use tokio::sync::RwLock;

/// Helper to build a raw (mcp-stream ...) form Expression mirroring the example file.
fn build_mcp_stream_form() -> Expression {
    // (mcp-stream "weather.monitor.v1" process-weather-chunk {:count 0})
    let head = Expression::Symbol(Symbol("mcp-stream".into()));
    let endpoint = Expression::Literal(Literal::String("weather.monitor.v1".into()));
    let processor = Expression::Symbol(Symbol("process-weather-chunk".into()));
    // initial-state map { :count 0 }
    let mut m = std::collections::HashMap::new();
    m.insert(MapKey::Keyword(Keyword("count".into())), Expression::Literal(Literal::Integer(0)));
    let init_state = Expression::Map(m);
    Expression::List(vec![head, endpoint, processor, init_state])
}

#[tokio::test]
async fn test_macro_lowering_and_stream_registration() {
    // Setup capability marketplace and register streaming capability
    let capability_registry = Arc::new(RwLock::new(rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(capability_registry));
    register_mcp_streaming_capability(marketplace.clone(), "http://localhost/mock".into()).await.expect("register capability");

    // Build raw macro form
    let raw = build_mcp_stream_form();
    let lowered = maybe_lower_mcp_stream_macro(&raw);

    // Expect lowered to be (call :mcp.stream.start { ... }) list of len 3
    match &lowered { 
        Expression::List(items) => {
            assert_eq!(items.len(), 3, "Lowered form should have 3 elements");
            match &items[0] { Expression::Symbol(Symbol(s)) => assert_eq!(s, "call"), _ => panic!("First element not call symbol") }
            match &items[1] { Expression::Literal(Literal::Keyword(Keyword(k))) => assert_eq!(k, "mcp.stream.start"), _ => panic!("Second element not capability keyword") }
            match &items[2] { Expression::Map(m) => {
                // Ensure endpoint & processor keys present
                let endpoint_key = MapKey::Keyword(Keyword("endpoint".into()));
                let processor_key = MapKey::Keyword(Keyword("processor".into()));
                assert!(m.contains_key(&endpoint_key));
                assert!(m.contains_key(&processor_key));
            }, _ => panic!("Third element must be map") }
        },
        _ => panic!("Lowered expression not a list")
    };
}

#[tokio::test]
async fn test_stream_start_and_mock_loop_processing() {
    // We'll directly instantiate the provider (bypassing marketplace execution path which is not yet wired here)
    let provider = McpStreamingProvider::new("http://localhost/mock".into());

    // Simulate lowered call parameter map used by start_stream
    use rtfs_compiler::ast::{MapKey, Keyword};
    let mut m = std::collections::HashMap::new();
    m.insert(MapKey::Keyword(Keyword("endpoint".into())), Value::String("weather.monitor.v1".into()));
    m.insert(MapKey::Keyword(Keyword("processor".into())), Value::String("process-weather-chunk".into()));
    m.insert(MapKey::Keyword(Keyword("initial-state".into())), Value::Map(std::collections::HashMap::new()));
    let params = Value::Map(m);

    // Start stream
    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    // Run mock loop to inject a few chunks; should not error
    let res = run_mock_stream_loop(&provider, stream_id.clone(), 5).await;
    assert!(res.is_ok(), "Mock loop failed: {:?}", res.err());

    // Validate that processor registration remains present
    let processors = provider.stream_processors.lock().unwrap();
    assert!(processors.get(&stream_id).is_some(), "Processor should remain registered after mock loop");
    drop(processors);

    // Assert state count incremented to 5
    let state = provider.get_current_state(&stream_id).expect("state present");
    if let Value::Map(m) = state {
        use rtfs_compiler::ast::{MapKey, Keyword};
        let key = MapKey::Keyword(Keyword("count".into()));
        let count_val = m.get(&key).and_then(|v| if let Value::Integer(i)=v { Some(*i) } else { None });
        assert_eq!(count_val, Some(5), "Expected :count == 5 after 5 chunks, got {:?}", count_val);
    } else {
        panic!("State not a map");
    }
}

#[tokio::test]
async fn test_directive_completion() {
    // provider & stream setup
    let provider = McpStreamingProvider::new("http://localhost/mock".into());
    let mut m = std::collections::HashMap::new();
    use rtfs_compiler::ast::{MapKey, Keyword};
    m.insert(MapKey::Keyword(Keyword("endpoint".into())), Value::String("weather.monitor.v1".into()));
    m.insert(MapKey::Keyword(Keyword("processor".into())), Value::String("process-weather-chunk".into()));
    m.insert(MapKey::Keyword(Keyword("initial-state".into())), Value::Map(std::collections::HashMap::new()));
    let params = Value::Map(m);
    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    // Send 2 normal chunks then a completion directive
    for seq in 0..2 {
        let mut chm = std::collections::HashMap::new();
        chm.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(seq));
        chm.insert(MapKey::Keyword(Keyword("payload".into())), Value::String(format!("normal-{}", seq)));
        let chunk = Value::Map(chm);
        provider.process_chunk(&stream_id, chunk, Value::Map(std::collections::HashMap::new())).await.expect("process");
    }
    // completion directive
    let mut cm = std::collections::HashMap::new();
    cm.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(99));
    cm.insert(MapKey::Keyword(Keyword("payload".into())), Value::String("final".into()));
    cm.insert(MapKey::Keyword(Keyword("action".into())), Value::Keyword(Keyword("complete".into())));
    let completion_chunk = Value::Map(cm);
    provider.process_chunk(&stream_id, completion_chunk, Value::Map(std::collections::HashMap::new())).await.expect("process completion");

    // attempt one more chunk which should be ignored (no state change after completion)
    let before_state = provider.get_current_state(&stream_id).unwrap();
    let mut ignored_map = std::collections::HashMap::new();
    ignored_map.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(100));
    ignored_map.insert(MapKey::Keyword(Keyword("payload".into())), Value::String("ignored".into()));
    let ignored_chunk = Value::Map(ignored_map);
    provider.process_chunk(&stream_id, ignored_chunk, Value::Map(std::collections::HashMap::new())).await.expect("process ignored");
    let after_state = provider.get_current_state(&stream_id).unwrap();
    assert_eq!(before_state.to_string(), after_state.to_string(), "State should not change after completion");
    assert_eq!(provider.get_status(&stream_id), Some(StreamStatus::Completed), "Stream should be marked completed");
}

#[tokio::test]
async fn test_directive_stop() {
    let provider = McpStreamingProvider::new("http://localhost/mock".into());
    use rtfs_compiler::ast::{MapKey, Keyword};
    let mut m = std::collections::HashMap::new();
    m.insert(MapKey::Keyword(Keyword("endpoint".into())), Value::String("weather.monitor.v1".into()));
    m.insert(MapKey::Keyword(Keyword("processor".into())), Value::String("process-weather-chunk".into()));
    m.insert(MapKey::Keyword(Keyword("initial-state".into())), Value::Map(std::collections::HashMap::new()));
    let params = Value::Map(m);
    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    // Send a normal chunk
    let mut c1 = std::collections::HashMap::new();
    c1.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(0));
    c1.insert(MapKey::Keyword(Keyword("payload".into())), Value::String("before-stop".into()));
    provider.process_chunk(&stream_id, Value::Map(c1), Value::Map(std::collections::HashMap::new())).await.unwrap();

    // Send stop directive
    let mut stop_map = std::collections::HashMap::new();
    stop_map.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(1));
    stop_map.insert(MapKey::Keyword(Keyword("payload".into())), Value::String("final".into()));
    stop_map.insert(MapKey::Keyword(Keyword("action".into())), Value::Keyword(Keyword("stop".into())));
    provider.process_chunk(&stream_id, Value::Map(stop_map), Value::Map(std::collections::HashMap::new())).await.unwrap();

    let status = provider.get_status(&stream_id).expect("status");
    assert_eq!(status, StreamStatus::Stopped, "Expected Stopped status");

    // Further chunk should be ignored (state snapshot won't change)
    let before_state = provider.get_current_state(&stream_id).unwrap();
    let mut ignored = std::collections::HashMap::new();
    ignored.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(2));
    ignored.insert(MapKey::Keyword(Keyword("payload".into())), Value::String("ignored".into()));
    provider.process_chunk(&stream_id, Value::Map(ignored), Value::Map(std::collections::HashMap::new())).await.unwrap();
    let after_state = provider.get_current_state(&stream_id).unwrap();
    assert_eq!(before_state.to_string(), after_state.to_string(), "State should not change after stop");
}

#[tokio::test]
async fn test_unknown_directive_sets_error() {
    let provider = McpStreamingProvider::new("http://localhost/mock".into());
    use rtfs_compiler::ast::{MapKey, Keyword};
    let mut m = std::collections::HashMap::new();
    m.insert(MapKey::Keyword(Keyword("endpoint".into())), Value::String("weather.monitor.v1".into()));
    m.insert(MapKey::Keyword(Keyword("processor".into())), Value::String("process-weather-chunk".into()));
    m.insert(MapKey::Keyword(Keyword("initial-state".into())), Value::Map(std::collections::HashMap::new()));
    let params = Value::Map(m);
    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    // Unknown directive chunk
    let mut unk = std::collections::HashMap::new();
    unk.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(0));
    unk.insert(MapKey::Keyword(Keyword("payload".into())), Value::String("mystery".into()));
    unk.insert(MapKey::Keyword(Keyword("action".into())), Value::Keyword(Keyword("explode".into()))); // not recognized
    provider.process_chunk(&stream_id, Value::Map(unk), Value::Map(std::collections::HashMap::new())).await.unwrap();

    if let Some(StreamStatus::Error(msg)) = provider.get_status(&stream_id) {
        assert!(msg.contains("Unknown action directive"), "Unexpected error msg: {}", msg);
    } else {
        panic!("Expected Error status with message");
    }

    // Further chunk should be ignored
    let before_state = provider.get_current_state(&stream_id).unwrap();
    let mut further = std::collections::HashMap::new();
    further.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(1));
    further.insert(MapKey::Keyword(Keyword("payload".into())), Value::String("ignored".into()));
    provider.process_chunk(&stream_id, Value::Map(further), Value::Map(std::collections::HashMap::new())).await.unwrap();
    let after_state = provider.get_current_state(&stream_id).unwrap();
    assert_eq!(before_state.to_string(), after_state.to_string(), "State should not change after error");
}
