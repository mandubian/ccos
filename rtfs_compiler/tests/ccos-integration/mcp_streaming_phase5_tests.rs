use rtfs_compiler::ast::{Keyword, MapKey};
use rtfs_compiler::runtime::{
    streaming::{
        InMemoryStreamPersistence, McpStreamingProvider, StreamStatus, StreamingCapability,
    },
    values::Value,
};
use std::sync::Arc;

fn make_params() -> Value {
    make_params_with_capacity(32)
}

fn make_params_with_capacity(capacity: usize) -> Value {
    let mut m = std::collections::HashMap::new();
    m.insert(
        MapKey::Keyword(Keyword("endpoint".into())),
        Value::String("weather.monitor.v1".into()),
    );
    m.insert(
        MapKey::Keyword(Keyword("processor".into())),
        Value::String("process-weather-chunk".into()),
    );
    m.insert(
        MapKey::Keyword(Keyword("initial-state".into())),
        Value::Map(std::collections::HashMap::new()),
    );
    m.insert(
        MapKey::Keyword(Keyword("queue-capacity".into())),
        Value::Integer(capacity as i64),
    );
    Value::Map(m)
}

fn make_chunk(seq: i64) -> Value {
    let mut m = std::collections::HashMap::new();
    m.insert(MapKey::Keyword(Keyword("seq".into())), Value::Integer(seq));
    Value::Map(m)
}

fn make_directive(action: &str) -> Value {
    let mut m = std::collections::HashMap::new();
    m.insert(
        MapKey::Keyword(Keyword("action".into())),
        Value::Keyword(Keyword(action.into())),
    );
    Value::Map(m)
}

#[tokio::test]
async fn test_queue_backpressure_and_pause_resume() {
    let persistence = Arc::new(InMemoryStreamPersistence::new());
    let provider = McpStreamingProvider::new_with_persistence(
        "http://localhost/mock".into(),
        persistence,
        None,
    );
    let params = make_params_with_capacity(5);
    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    provider
        .process_chunk(
            &stream_id,
            make_directive("pause"),
            Value::Map(std::collections::HashMap::new()),
        )
        .await
        .expect("pause directive");
    assert_eq!(provider.get_status(&stream_id), Some(StreamStatus::Paused));

    for seq in 0..5 {
        provider
            .process_chunk(
                &stream_id,
                make_chunk(seq),
                Value::Map(std::collections::HashMap::new()),
            )
            .await
            .expect("enqueue while paused");
    }

    let overflow = provider
        .process_chunk(
            &stream_id,
            make_chunk(99),
            Value::Map(std::collections::HashMap::new()),
        )
        .await;
    assert!(overflow.is_err(), "expected queue overflow error");

    provider
        .process_chunk(
            &stream_id,
            make_directive("resume"),
            Value::Map(std::collections::HashMap::new()),
        )
        .await
        .expect("resume directive");
    assert_eq!(provider.get_status(&stream_id), Some(StreamStatus::Active));
}

#[tokio::test]
async fn test_cancel_directive_stops_processing() {
    let provider = McpStreamingProvider::new("http://localhost/mock".into());
    let params = make_params();
    let handle = provider.start_stream(&params).expect("start stream");
    let stream_id = handle.stream_id.clone();

    provider
        .process_chunk(
            &stream_id,
            make_directive("cancel"),
            Value::Map(std::collections::HashMap::new()),
        )
        .await
        .expect("cancel directive");

    assert_eq!(
        provider.get_status(&stream_id),
        Some(StreamStatus::Cancelled)
    );

    provider
        .process_chunk(
            &stream_id,
            make_chunk(99),
            Value::Map(std::collections::HashMap::new()),
        )
        .await
        .expect("enqueue after cancel should be no-op");
}
