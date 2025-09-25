use std::collections::HashMap;
use std::time::Duration;

use rtfs_compiler::ast::{Keyword, MapKey};
use rtfs_compiler::runtime::streaming::{
    McpStreamingProvider, StreamingCapability, DEFAULT_LOCAL_MCP_SSE_ENDPOINT,
    ENV_LEGACY_CLOUDFLARE_DOCS_SSE_URL, ENV_LOCAL_MCP_SSE_URL, ENV_MCP_STREAM_AUTH_HEADER,
    ENV_MCP_STREAM_AUTO_CONNECT, ENV_MCP_STREAM_BEARER_TOKEN, ENV_MCP_STREAM_ENDPOINT,
};
use rtfs_compiler::runtime::values::Value;
use tokio::time::{sleep, Instant};

const LIVE_DATASET_ENDPOINT: &str = "weather.monitor.v1";

fn build_live_params() -> Value {
    let mut params = HashMap::new();
    params.insert(
        MapKey::Keyword(Keyword("endpoint".into())),
        Value::String(LIVE_DATASET_ENDPOINT.into()),
    );
    params.insert(
        MapKey::Keyword(Keyword("processor".into())),
        Value::String("process-weather-chunk".into()),
    );

    let mut state_map = HashMap::new();
    state_map.insert(MapKey::Keyword(Keyword("count".into())), Value::Integer(0));
    params.insert(
        MapKey::Keyword(Keyword("initial-state".into())),
        Value::Map(state_map),
    );

    Value::Map(params)
}

fn store_env_var(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn restore_env_var(key: &str, previous: Option<String>) {
    if let Some(value) = previous {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn should_run_live_test() -> bool {
    match std::env::var("CCOS_RUN_MCP_LIVE_TESTS") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

#[tokio::test]
async fn test_live_local_mcp_stream() {
    if !should_run_live_test() {
        eprintln!(
            "Skipping live MCP SSE test – set CCOS_RUN_MCP_LIVE_TESTS=1 to enable (ensure local server running)"
        );
        return;
    }

    let prev_endpoint = store_env_var(ENV_MCP_STREAM_ENDPOINT);
    let prev_local_url = store_env_var(ENV_LOCAL_MCP_SSE_URL);
    let prev_legacy_url = store_env_var(ENV_LEGACY_CLOUDFLARE_DOCS_SSE_URL);
    let prev_auth_header = store_env_var(ENV_MCP_STREAM_AUTH_HEADER);
    let prev_bearer = store_env_var(ENV_MCP_STREAM_BEARER_TOKEN);
    let prev_auto_connect = store_env_var(ENV_MCP_STREAM_AUTO_CONNECT);

    std::env::set_var(ENV_MCP_STREAM_ENDPOINT, DEFAULT_LOCAL_MCP_SSE_ENDPOINT);
    std::env::remove_var(ENV_LOCAL_MCP_SSE_URL);
    std::env::remove_var(ENV_LEGACY_CLOUDFLARE_DOCS_SSE_URL);
    std::env::remove_var(ENV_MCP_STREAM_AUTH_HEADER);
    std::env::remove_var(ENV_MCP_STREAM_BEARER_TOKEN);
    std::env::remove_var(ENV_MCP_STREAM_AUTO_CONNECT);

    let provider = McpStreamingProvider::new(String::new());
    let params = build_live_params();
    let handle = provider
        .start_stream(&params)
        .expect("failed to start live MCP stream");
    let stream_id = handle.stream_id.clone();

    let deadline = Instant::now() + Duration::from_secs(30);
    let target_chunks = 2;
    let mut observed_chunks = 0;

    while Instant::now() < deadline {
        if let Some(Value::Map(state_map)) = provider.get_current_state(&stream_id) {
            if let Some(Value::Integer(count)) =
                state_map.get(&MapKey::Keyword(Keyword("count".into())))
            {
                if *count > observed_chunks {
                    observed_chunks = *count;
                    println!("Observed {} chunk(s) so far", observed_chunks);
                }
                if *count >= target_chunks {
                    break;
                }
            }
        }
        sleep(Duration::from_millis(500)).await;
    }

    if let Some(state) = provider.get_current_state(&stream_id) {
        println!("Final stream state: {}", state);
    }

    provider
        .stop_stream(&handle)
        .expect("failed to stop live MCP stream");

    restore_env_var(ENV_MCP_STREAM_ENDPOINT, prev_endpoint);
    restore_env_var(ENV_LOCAL_MCP_SSE_URL, prev_local_url);
    restore_env_var(ENV_LEGACY_CLOUDFLARE_DOCS_SSE_URL, prev_legacy_url);
    restore_env_var(ENV_MCP_STREAM_AUTH_HEADER, prev_auth_header);
    restore_env_var(ENV_MCP_STREAM_BEARER_TOKEN, prev_bearer);
    restore_env_var(ENV_MCP_STREAM_AUTO_CONNECT, prev_auto_connect);

    assert!(
        observed_chunks >= target_chunks,
        "Expected to receive at least {} SSE message(s) from local MCP endpoint, observed {}",
        target_chunks,
        observed_chunks
    );
}
