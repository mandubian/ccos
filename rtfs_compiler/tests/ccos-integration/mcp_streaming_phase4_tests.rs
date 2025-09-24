use std::sync::Arc;
use rtfs_compiler::runtime::{
    streaming::{InMemoryStreamPersistence, McpStreamingProvider, StreamingCapability},
    values::Value,
};

#[tokio::test]
async fn test_stream_persists_and_resumes_state() {
    let persistence = Arc::new(InMemoryStreamPersistence::new());
    let provider = McpStreamingProvider::new_with_persistence(
        "http://localhost/mock".into(),
        persistence.clone(),
        None,
    );

    use rtfs_compiler::ast::{Keyword, MapKey};
    let mut map = std::collections::HashMap::new();
    map.insert(MapKey::Keyword(Keyword("endpoint".into())), Value::String("weather.monitor.v1".into()));
    map.insert(MapKey::Keyword(Keyword("processor".into())), Value::String("process-weather-chunk".into()));
    let mut initial_state_map = std::collections::HashMap::new();
    initial_state_map.insert(MapKey::Keyword(Keyword("count".into())), Value::Integer(0));
    map.insert(MapKey::Keyword(Keyword("initial-state".into())), Value::Map(initial_state_map));
    let params = Value::Map(map);

    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    let mut chunk_map = std::collections::HashMap::new();
    chunk_map.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(0));
    let chunk = Value::Map(chunk_map);
    provider
        .process_chunk(&stream_id, chunk, Value::Map(std::collections::HashMap::new()))
        .await
        .expect("process chunk");

    drop(provider);

    let new_provider = McpStreamingProvider::new_with_persistence(
        "http://localhost/mock".into(),
        persistence.clone(),
        None,
    );

    new_provider
        .resume_stream(&stream_id)
        .expect("resume stream from snapshot");

    let state = new_provider
        .get_current_state(&stream_id)
        .expect("state after resume");
    if let Value::Map(state_map) = state {
        let key = MapKey::Keyword(Keyword("count".into()));
        let count_val = state_map
            .get(&key)
            .and_then(|v| if let Value::Integer(i) = v { Some(*i) } else { None });
        assert_eq!(count_val, Some(1), "Expected persisted count == 1");
    } else {
        panic!("Expected map state after resume");
    }
}

#[tokio::test]
async fn test_resume_missing_snapshot_errors() {
    let persistence = Arc::new(InMemoryStreamPersistence::new());
    let provider = McpStreamingProvider::new_with_persistence(
        "http://localhost/mock".into(),
        persistence,
        None,
    );

    let err = provider.resume_stream("missing-stream");
    assert!(err.is_err(), "Expected error when snapshot missing");
}
