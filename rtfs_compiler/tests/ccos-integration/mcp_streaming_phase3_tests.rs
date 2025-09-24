use std::sync::Arc;
use rtfs_compiler::runtime::{
    streaming::{McpStreamingProvider, StreamStatus, StreamingCapability},
    values::Value,
    error::{RuntimeError, RuntimeResult},
};
use rtfs_compiler::ast::{MapKey, Keyword};

// NOTE:
// Originally these tests attempted to invoke a real RTFS function via Evaluator.
// That required accessing internal test helpers behind cfg(test) and introduced
// Send/Sync trait bound issues for HostInterface inside Evaluator when placed
// behind the Arc<dyn Fn + Send + Sync> invoker. For Phase 3 coverage we only
// need to validate McpStreamingProvider's interpretation of processor return
// maps. We therefore simulate the "processor function" directly inside the
// invoker closure. This keeps tests simple while still exercising:
//  - incrementing state based on incoming chunk
//  - emitting a completion directive when a condition is met (seq == 3)
//  - providing an :output key (ignored but recognized)
//  - invalid return shape error handling
// If future refactors expose a safe test evaluator with Send + Sync host, the
// real interpreter path can be reintroduced without changing provider code.

#[tokio::test]
async fn test_phase3_processor_invocation() {
    // Simulated processor name
    let fn_name = "process-weather-chunk".to_string();

    // Simulated invocation closure emulating the RTFS function semantics.
    let invoker = Arc::new(move |name: &str, state: &Value, chunk: &Value, _metadata: &Value| -> RuntimeResult<Value> {
        if name != "process-weather-chunk" {
            return Err(RuntimeError::Generic(format!("unknown processor: {}", name)));
        }
        // Extract current count
        let mut count = 0;
        if let Value::Map(m) = state {
            let k = MapKey::Keyword(Keyword("count".into()));
            if let Some(Value::Integer(i)) = m.get(&k) { count = *i; }
        }
        let new_count = count + 1;
        // Extract seq from chunk
        let seq = if let Value::Map(m) = chunk {
            let k = MapKey::Keyword(Keyword("seq".into()));
            if let Some(Value::Integer(i)) = m.get(&k) { *i } else { -1 }
        } else { -1 };
        // Build new state map
        let mut new_state_map = std::collections::HashMap::new();
        new_state_map.insert(MapKey::Keyword(Keyword("count".into())), Value::Integer(new_count));
        let mut result_map = std::collections::HashMap::new();
        result_map.insert(MapKey::Keyword(Keyword("state".into())), Value::Map(new_state_map));
        if seq == 3 { // completion condition
            result_map.insert(MapKey::Keyword(Keyword("action".into())), Value::Keyword(Keyword("complete".into())));
            // include :output to ensure it's gracefully recognized
            let mut out = std::collections::HashMap::new();
            out.insert(MapKey::Keyword(Keyword("final-count".into())), Value::Integer(new_count));
            result_map.insert(MapKey::Keyword(Keyword("output".into())), Value::Map(out));
        }
        Ok(Value::Map(result_map))
    });

    let provider = McpStreamingProvider::new_with_invoker("http://localhost/mock".into(), invoker);

    // Start stream with initial state {:count 0}
    let mut param_map = std::collections::HashMap::new();
    param_map.insert(MapKey::Keyword(Keyword("endpoint".into())), Value::String("weather.monitor.v1".into()));
    param_map.insert(MapKey::Keyword(Keyword("processor".into())), Value::String(fn_name));
    let mut init_state = std::collections::HashMap::new();
    init_state.insert(MapKey::Keyword(Keyword("count".into())), Value::Integer(0));
    param_map.insert(MapKey::Keyword(Keyword("initial-state".into())), Value::Map(init_state));
    let params = Value::Map(param_map);
    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    // Feed 4 chunks, last should cause completion directive via processor return
    for seq in 0..4 { // 0,1,2,3
        let mut ch = std::collections::HashMap::new();
        ch.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(seq));
        ch.insert(MapKey::Keyword(Keyword("payload".into())), Value::String(format!("chunk-{}", seq)));
        let chunk = Value::Map(ch);
        provider.process_chunk(&stream_id, chunk, Value::Map(std::collections::HashMap::new())).await.expect("process");
    }

    // After 4 chunks, :count should be 4 and status should be Completed (directive from function when seq==3)
    let state = provider.get_current_state(&stream_id).expect("state");
    if let Value::Map(m) = state { 
        let key = MapKey::Keyword(Keyword("count".into()));
        let c = m.get(&key).and_then(|v| if let Value::Integer(i)=v {Some(*i)} else {None});
        assert_eq!(c, Some(4), "Expected :count 4 after 4 invocations");
    } else { panic!("State not map"); }
    assert_eq!(provider.get_status(&stream_id), Some(StreamStatus::Completed), "Expected Completed status from processor directive");
}

#[tokio::test]
async fn test_phase3_invalid_return_shape_errors() {
    // Bad processor returning non-map to trigger error path
    let invoker = Arc::new(|name: &str, _state: &Value, _chunk: &Value, _metadata: &Value| -> RuntimeResult<Value> {
        if name == "bad-processor" { Ok(Value::Integer(42)) } else { Err(RuntimeError::Generic("missing".into())) }
    });
    let provider = McpStreamingProvider::new_with_invoker("http://localhost/mock".into(), invoker);
    let mut param_map = std::collections::HashMap::new();
    param_map.insert(MapKey::Keyword(Keyword("endpoint".into())), Value::String("weather.monitor.v1".into()));
    param_map.insert(MapKey::Keyword(Keyword("processor".into())), Value::String("bad-processor".into()));
    param_map.insert(MapKey::Keyword(Keyword("initial-state".into())), Value::Map(std::collections::HashMap::new()));
    let params = Value::Map(param_map);
    let handle = provider.start_stream(&params).unwrap();
    let stream_id = handle.stream_id.clone();

    // One chunk should trigger error
    let mut ch = std::collections::HashMap::new();
    ch.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(0));
    let chunk = Value::Map(ch);
    let err = provider.process_chunk(&stream_id, chunk, Value::Map(std::collections::HashMap::new())).await.err();
    assert!(err.is_some(), "Expected error for invalid return shape");
    if let Some(StreamStatus::Error(msg)) = provider.get_status(&stream_id) { assert!(msg.contains("invalid shape"), "Unexpected msg: {}", msg); } else { panic!("Expected Error status"); }
}
