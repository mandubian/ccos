//! Mock Moltbook Server for Testing
//!
//! A simple HTTP server that simulates Moltbook API endpoints for testing
//! the skill onboarding system without requiring real registration.
//!
//! Usage:
//!   cargo run --bin mock-moltbook
//!
//! Endpoints:
//!   POST /api/register-agent    - Register a new agent (returns agent_id + secret)
//!   POST /api/human-claim       - Initiate human claim (returns tweet text)
//!   POST /api/verify-human-claim - Verify tweet was posted
//!   POST /api/setup-heartbeat   - Setup agent heartbeat
//!   POST /api/post-to-feed      - Post to feed (requires verified agent)
//!
//! The server maintains state in memory and prints all requests/responses
//! so you can see exactly what's happening.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use uuid::Uuid;

/// In-memory state for the mock server
#[derive(Clone)]
struct MockState {
    agents: Arc<Mutex<HashMap<String, Agent>>>,
    verifications: Arc<Mutex<HashMap<String, VerificationStatus>>>,
    posts: Arc<Mutex<Vec<Post>>>,
}

#[derive(Serialize, Clone)]
struct Post {
    id: String,
    agent_id: String,
    content: String,
    timestamp: String,
}

struct Agent {
    id: String,
    name: String,
    #[allow(dead_code)]
    model: String,
    secret: String,
    human_x_username: Option<String>,
    verified: bool,
    #[allow(dead_code)]
    created_at: String,
}

struct VerificationStatus {
    #[allow(dead_code)]
    agent_id: String,
    #[allow(dead_code)]
    tweet_text: String,
    tweet_url: Option<String>,
    verified: bool,
}

/// Register agent request
#[derive(Deserialize)]
struct RegisterAgentRequest {
    name: String,
    model: String,
    #[serde(default)]
    #[allow(dead_code)]
    created_by: Option<String>,
}

/// Register agent response
#[derive(Serialize)]
struct RegisterAgentResponse {
    agent_id: String,
    secret: String,
    message: String,
}

/// Human claim request
#[derive(Deserialize)]
struct HumanClaimRequest {
    human_x_username: String,
}

/// Human claim response
#[derive(Serialize)]
struct HumanClaimResponse {
    verification_tweet_text: String,
    message: String,
}

/// Verify human claim request
#[derive(Deserialize)]
struct VerifyHumanClaimRequest {
    tweet_url: String,
}

/// Verify human claim response
#[derive(Serialize)]
struct VerifyHumanClaimResponse {
    success: bool,
    message: String,
}

/// Setup heartbeat request
#[derive(Deserialize)]
struct SetupHeartbeatRequest {
    prompt_id: String,
    interval_hours: u32,
}

/// Setup heartbeat response
#[derive(Serialize)]
struct SetupHeartbeatResponse {
    success: bool,
    message: String,
}

/// Post to feed request
#[derive(Deserialize)]
struct PostToFeedRequest {
    content: String,
}

/// Post to feed response
#[derive(Serialize)]
struct PostToFeedResponse {
    success: bool,
    post_id: String,
    message: String,
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "Mock Moltbook Server is running!\n"
}

/// Register a new agent
async fn register_agent(
    State(state): State<MockState>,
    Json(req): Json<RegisterAgentRequest>,
) -> Result<Json<RegisterAgentResponse>, StatusCode> {
    let agent_id = format!(
        "moltbook_agent_{}",
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let secret = format!("sk_molt_{}", Uuid::new_v4().to_string().replace("-", ""));

    let agent = Agent {
        id: agent_id.clone(),
        name: req.name.clone(),
        model: req.model.clone(),
        secret: secret.clone(),
        human_x_username: None,
        verified: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    state.agents.lock().unwrap().insert(agent_id.clone(), agent);

    println!("\n[REGISTER-AGENT] New agent registered:");
    println!("  Name: {}", req.name);
    println!("  Model: {}", req.model);
    println!("  Agent ID: {}", agent_id);
    println!("  Secret: {} (store this securely!)\n", secret);

    Ok(Json(RegisterAgentResponse {
        agent_id,
        secret,
        message: "Agent registered successfully. Store the secret securely!".to_string(),
    }))
}

/// Initiate human claim process
async fn human_claim(
    State(state): State<MockState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<HumanClaimRequest>,
) -> Result<Json<HumanClaimResponse>, StatusCode> {
    // Extract authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let secret = auth_header.strip_prefix("Bearer ").unwrap_or("");

    // Find agent by secret
    let agents = state.agents.lock().unwrap();
    let agent = agents
        .values()
        .find(|a| a.secret == secret)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let agent_id = agent.id.clone();
    drop(agents); // Release lock

    // Generate verification tweet
    let code = Uuid::new_v4()
        .to_string()
        .split('-')
        .next()
        .unwrap()
        .to_uppercase();
    let tweet_text = format!(
        "I'm verifying my AI agent {} on @moltbook. Verification code: {}",
        agent_id, code
    );

    // Store verification status
    let verification = VerificationStatus {
        agent_id: agent_id.clone(),
        tweet_text: tweet_text.clone(),
        tweet_url: None,
        verified: false,
    };

    state
        .verifications
        .lock()
        .unwrap()
        .insert(agent_id.clone(), verification);

    // Update agent with human username
    if let Some(agent) = state.agents.lock().unwrap().get_mut(&agent_id) {
        agent.human_x_username = Some(req.human_x_username.clone());
    }

    println!("\n[HUMAN-CLAIM] Human claim initiated:");
    println!("  Agent ID: {}", agent_id);
    println!("  Human X Username: {}", req.human_x_username);
    println!("  Verification Tweet: {}", tweet_text);
    println!("  Instructions: Post this tweet from your X account, then call verify-human-claim with the tweet URL\n");

    Ok(Json(HumanClaimResponse {
        verification_tweet_text: tweet_text,
        message: "Please post this tweet from your X account and then verify".to_string(),
    }))
}

/// Verify human claim
async fn verify_human_claim(
    State(state): State<MockState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<VerifyHumanClaimRequest>,
) -> Result<Json<VerifyHumanClaimResponse>, StatusCode> {
    // Extract authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let secret = auth_header.strip_prefix("Bearer ").unwrap_or("");

    // Find agent by secret
    let agents = state.agents.lock().unwrap();
    let agent = agents
        .values()
        .find(|a| a.secret == secret)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let agent_id = agent.id.clone();
    drop(agents);

    // Update verification status
    let mut verifications = state.verifications.lock().unwrap();
    if let Some(verification) = verifications.get_mut(&agent_id) {
        verification.tweet_url = Some(req.tweet_url.clone());
        verification.verified = true;
    }
    drop(verifications);

    // Mark agent as verified
    if let Some(agent) = state.agents.lock().unwrap().get_mut(&agent_id) {
        agent.verified = true;
    }

    println!("\n[VERIFY-HUMAN-CLAIM] Human claim verified:");
    println!("  Agent ID: {}", agent_id);
    println!("  Tweet URL: {}", req.tweet_url);
    println!("  Status: ✅ VERIFIED\n");

    Ok(Json(VerifyHumanClaimResponse {
        success: true,
        message: "Human claim verified successfully. Agent is now fully operational!".to_string(),
    }))
}

/// Setup heartbeat
async fn setup_heartbeat(
    State(state): State<MockState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SetupHeartbeatRequest>,
) -> Result<Json<SetupHeartbeatResponse>, StatusCode> {
    // Extract authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let secret = auth_header.strip_prefix("Bearer ").unwrap_or("");

    // Find agent by secret
    let agents = state.agents.lock().unwrap();
    let agent = agents
        .values()
        .find(|a| a.secret == secret)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let agent_id = agent.id.clone();
    let is_verified = agent.verified;
    drop(agents);

    if !is_verified {
        return Ok(Json(SetupHeartbeatResponse {
            success: false,
            message: "Agent must be verified before setting up heartbeat".to_string(),
        }));
    }

    println!("\n[SETUP-HEARTBEAT] Heartbeat configured:");
    println!("  Agent ID: {}", agent_id);
    println!("  Prompt ID: {}", req.prompt_id);
    println!("  Interval: {} hours\n", req.interval_hours);

    Ok(Json(SetupHeartbeatResponse {
        success: true,
        message: format!(
            "Heartbeat setup successfully. Will run every {} hours",
            req.interval_hours
        ),
    }))
}

/// Post to feed
async fn post_to_feed(
    State(state): State<MockState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<PostToFeedRequest>,
) -> Result<Json<PostToFeedResponse>, StatusCode> {
    // Extract authorization header
    // Extract authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let (agent_id, is_verified) = if auth_header.is_empty() {
        ("system".to_string(), true)
    } else {
        let secret = auth_header.strip_prefix("Bearer ").unwrap_or("");
        // Find agent by secret
        let agents = state.agents.lock().unwrap();
        if let Some(agent) = agents.values().find(|a| a.secret == secret) {
            (agent.id.clone(), agent.verified)
        } else {
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    if !is_verified && agent_id != "system" {
        return Ok(Json(PostToFeedResponse {
            success: false,
            post_id: "".to_string(),
            message: "Agent must be verified before posting".to_string(),
        }));
    }

    let post_id = format!(
        "post_{}",
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );

    let post = Post {
        id: post_id.clone(),
        agent_id: agent_id.clone(),
        content: req.content.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    state.posts.lock().unwrap().push(post);

    println!("\n[POST-TO-FEED] New post created:");
    println!("  Agent ID: {}", agent_id);
    println!("  Post ID: {}", post_id);
    println!("  Content: {}\n", req.content);

    Ok(Json(PostToFeedResponse {
        success: true,
        post_id,
        message: "Posted to feed successfully".to_string(),
    }))
}

/// Get server status (for testing)
async fn server_status(State(state): State<MockState>) -> Json<serde_json::Value> {
    let agents = state.agents.lock().unwrap();
    let verifications = state.verifications.lock().unwrap();

    let agent_list: Vec<serde_json::Value> = agents
        .values()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "name": a.name,
                "verified": a.verified,
                "human_x_username": a.human_x_username,
            })
        })
        .collect();

    Json(serde_json::json!({
        "total_agents": agents.len(),
        "verified_agents": agents.values().filter(|a| a.verified).count(),
        "agents": agent_list,
        "pending_verifications": verifications.values().filter(|v| !v.verified).count(),
        "posts": *state.posts.lock().unwrap(),
    }))
}

/// Serve the skill.md file
async fn serve_skill_md() -> Result<
    (
        axum::http::StatusCode,
        axum::http::header::HeaderMap,
        String,
    ),
    StatusCode,
> {
    // The skill.md content embedded in the binary
    println!("[MOCK] Serving skill.md");
    let skill_md_content = include_str!("mock_moltbook_skill.md");

    let mut headers = axum::http::header::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::header::HeaderValue::from_static("text/markdown; charset=utf-8"),
    );

    Ok((StatusCode::OK, headers, skill_md_content.to_string()))
}

#[tokio::main]
async fn main() {
    println!("🚀 Starting Mock Moltbook Server...");
    println!("   This is a test server that simulates Moltbook APIs");
    println!("   All data is stored in memory and will be lost on restart\n");

    let state = MockState {
        agents: Arc::new(Mutex::new(HashMap::new())),
        verifications: Arc::new(Mutex::new(HashMap::new())),
        posts: Arc::new(Mutex::new(Vec::new())),
    };

    let app = Router::new()
        .route("/", get(health_check))
        .route("/skill.md", get(serve_skill_md))
        .route("/api/register-agent", post(register_agent))
        .route("/api/human-claim", post(human_claim))
        .route("/api/verify-human-claim", post(verify_human_claim))
        .route("/api/setup-heartbeat", post(setup_heartbeat))
        .route("/api/post-to-feed", post(post_to_feed))
        .route("/status", get(server_status))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8765));
    println!("📡 Server running on http://{}", addr);
    println!("   (Accessible from any network interface)");
    println!("\n📋 Available endpoints:");
    println!("   GET  /                   - Health check");
    println!("   GET  /status             - Server status (list agents)");
    println!("   POST /api/register-agent - Register new agent");
    println!("   POST /api/human-claim    - Initiate human verification");
    println!("   POST /api/verify-human-claim - Verify tweet posted");
    println!("   POST /api/setup-heartbeat - Configure agent heartbeat");
    println!("   POST /api/post-to-feed   - Post to Moltbook feed (verified only)");
    println!("\n💡 Tip: Watch this terminal to see all requests and responses!\n");

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
