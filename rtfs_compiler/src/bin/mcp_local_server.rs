use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::time::sleep;

const SERVER_SOURCE: &str = "mcp-local-server";

#[derive(Parser, Debug)]
#[command(version, about = "MCP local streaming server for CCOS testing", author)]
struct Args {
    /// Interface to bind (default localhost).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on for HTTP + SSE traffic.
    #[arg(long, default_value_t = 2025)]
    port: u16,

    /// Default interval between SSE chunk messages in milliseconds.
    #[arg(long, default_value_t = 800)]
    interval_ms: u64,

    /// Interval for SSE keep-alive pings in seconds.
    #[arg(long, default_value_t = 20)]
    keep_alive_secs: u64,
}

#[derive(Clone)]
struct ServerState {
    default_interval: Duration,
    keep_alive: Duration,
}

#[derive(Default, Deserialize)]
struct StreamQuery {
    limit: Option<usize>,
    interval_ms: Option<u64>,
}

#[derive(Default, Deserialize)]
struct DatasetQuery {
    limit: Option<usize>,
}

struct StreamLoopState {
    index: usize,
    completed: bool,
    interval: Duration,
    endpoint: String,
    chunks: Arc<Vec<JsonValue>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let state = ServerState {
        default_interval: Duration::from_millis(args.interval_ms),
        keep_alive: Duration::from_secs(args.keep_alive_secs),
    };

    let app = Router::new()
        .route("/", get(home))
        .route("/healthz", get(health))
        .route("/datasets", get(list_datasets))
        .route("/datasets/:endpoint", get(dataset))
        .route("/sse/:endpoint", get(stream_endpoint))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    println!(
        "Starting MCP local server on http://{} (SSE base http://{}/sse)",
        addr, addr
    );

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    println!("Local MCP server stopped");
    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = signal::ctrl_c().await {
        eprintln!("Failed to install Ctrl+C handler: {}", err);
        return;
    }
    println!("Ctrl+C received, shutting down...");
}

async fn home() -> Html<String> {
    let body = format!(
        "<html><body><h1>MCP Local Streaming Server</h1>\
         <p>Available endpoints: {}</p>\
         <p>Example SSE URL: <code>http://127.0.0.1:2025/sse/weather.monitor.v1</code></p>\
         </body></html>",
        available_endpoints().join(", ")
    );
    Html(body)
}

async fn health() -> impl IntoResponse {
    StatusCode::OK
}

async fn list_datasets(State(state): State<ServerState>) -> Json<JsonValue> {
    Json(json!({
        "source": SERVER_SOURCE,
        "endpoints": available_endpoints(),
        "default_interval_ms": state.default_interval.as_millis(),
    }))
}

async fn dataset(
    Path(endpoint): Path<String>,
    Query(query): Query<DatasetQuery>,
) -> Result<Json<JsonValue>, (StatusCode, Json<JsonValue>)> {
    let mut chunks = sample_dataset(&endpoint);
    if chunks.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("unknown endpoint '{}'", endpoint),
                "available": available_endpoints(),
            })),
        ));
    }
    if let Some(limit) = query.limit {
        if limit < chunks.len() {
            chunks.truncate(limit);
        }
    }
    Ok(Json(json!({
        "endpoint": endpoint,
        "chunk_count": chunks.len(),
        "chunks": chunks,
    })))
}

async fn stream_endpoint(
    State(state): State<ServerState>,
    Path(endpoint): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let mut chunks = sample_dataset(&endpoint);
    if chunks.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    if let Some(limit) = query.limit {
        if limit < chunks.len() {
            chunks.truncate(limit);
        }
    }

    let interval = query
        .interval_ms
        .map(|ms| Duration::from_millis(ms))
        .unwrap_or(state.default_interval);

    let stream_state = StreamLoopState {
        index: 0,
        completed: false,
        interval,
        endpoint: endpoint.clone(),
        chunks: Arc::new(chunks),
    };

    let keep_alive = state.keep_alive;
    let sse_stream = futures::stream::unfold(stream_state, |mut current| async move {
        if current.index < current.chunks.len() {
            if current.index > 0 {
                sleep(current.interval).await;
            }
            let chunk = &current.chunks[current.index];
            let payload = json!({
                "endpoint": current.endpoint,
                "seq": current.index,
                "chunk": chunk,
                "generated_at": chrono::Utc::now().to_rfc3339(),
                "trace": {
                    "source": SERVER_SOURCE,
                    "version": env!("CARGO_PKG_VERSION"),
                }
            });
            let event = Event::default()
                .id(current.index.to_string())
                .event("chunk")
                .data(payload.to_string());
            current.index += 1;
            Some((Ok(event), current))
        } else if !current.completed {
            sleep(current.interval).await;
            let payload = json!({
                "endpoint": current.endpoint,
                "status": "complete",
                "chunks_sent": current.chunks.len(),
                "trace": {
                    "source": SERVER_SOURCE,
                    "version": env!("CARGO_PKG_VERSION"),
                }
            });
            let event = Event::default().event("status").data(payload.to_string());
            current.completed = true;
            Some((Ok(event), current))
        } else {
            None
        }
    });

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(keep_alive).text("keep-alive")))
}

fn available_endpoints() -> Vec<&'static str> {
    vec!["weather.monitor.v1", "docs.search.v1", "news.digest.v1"]
}

fn sample_dataset(endpoint: &str) -> Vec<JsonValue> {
    match endpoint {
        "weather.monitor.v1" => weather_chunks(),
        "docs.search.v1" => docs_chunks(),
        "news.digest.v1" => news_chunks(),
        other => fallback_chunks(other),
    }
}

fn weather_chunks() -> Vec<JsonValue> {
    let cities = [
        "Paris",
        "Berlin",
        "London",
        "San Francisco",
        "Tokyo",
        "Sydney",
    ];
    let conditions = ["clear", "cloudy", "wind", "rain", "sunny", "fog"];
    cities
        .iter()
        .enumerate()
        .map(|(idx, city)| {
            let temperature = 20.0 + (idx as f64) * 1.5;
            let humidity = 55 + idx as i32 * 3;
            let condition = conditions[idx % conditions.len()];
            json!({
                "city": city,
                "temperature_c": temperature,
                "humidity_pct": humidity,
                "conditions": condition,
            })
        })
        .collect()
}

fn docs_chunks() -> Vec<JsonValue> {
    vec![
        json!({
            "q": "mcp streaming",
            "hits": [
                {"title": "Streaming Overview", "url": "https://example.local/docs/streaming"},
                {"title": "Chunk Lifecycle", "url": "https://example.local/docs/chunk"}
            ],
        }),
        json!({
            "q": "rtfs integration",
            "hits": [
                {"title": "RTFS 2.0 Guide", "url": "https://example.local/docs/rtfs"},
                {"title": "Effect Boundaries", "url": "https://example.local/docs/effects"}
            ],
        }),
    ]
}

fn news_chunks() -> Vec<JsonValue> {
    vec![
        json!({
            "headline": "New MCP Local Server Released",
            "summary": "A lightweight reference implementation for SSE-based streaming tests.",
        }),
        json!({
            "headline": "CCOS Streaming Phase 6",
            "summary": "Local development now defaults to an offline MCP endpoint for reliability.",
        }),
        json!({
            "headline": "RTFS Continues Rapid Iteration",
            "summary": "Delegation, governance, and streaming improvements land in latest release.",
        }),
    ]
}

fn fallback_chunks(endpoint: &str) -> Vec<JsonValue> {
    vec![json!({
        "note": "No curated dataset for this endpoint; emitting synthesized data.",
        "endpoint": endpoint,
    })]
}
