//! CCOS Chat CLI Tool
//!
//! Interactive CLI to talk to CCOS Chat Gateway.
//! It sends messages to the gateway's loopback connector and
//! optionally polls an external status endpoint to see agent responses.

use clap::Parser;
use colored::*;
use reqwest::Client;
use serde_json::json;
use std::io::{self, Write};
use tokio::time::{sleep, Duration};

#[derive(Parser, Debug)]
#[command(name = "ccos-chat")]
struct Args {
    #[arg(long, default_value = "http://localhost:8833")]
    connector_url: String,

    /// Optional external status endpoint to poll for posts (e.g., http://localhost:8765)
    #[arg(long)]
    status_url: Option<String>,

    #[arg(long, default_value = "demo-secret-key")]
    secret: String,

    #[arg(long, default_value = "user1")]
    user_id: String,

    #[arg(long, default_value = "chat-demo")]
    channel_id: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = Client::new();

    println!("{}", "=========================================".blue());
    println!(
        "{}",
        "       CCOS Interactive Chat Tool        ".blue().bold()
    );
    println!("{}", "=========================================".blue());
    println!("Connector: {}", args.connector_url.yellow());
    if let Some(ref status_url) = args.status_url {
        println!("Status:    {}", status_url.yellow());
    }
    println!("Channel:   {}", args.channel_id.green());
    println!("User:      {}", args.user_id.green());
    println!(
        "{}",
        "Type '@agent <message>' to talk to the agent.".dimmed()
    );
    println!("{}", "Type 'exit' or 'quit' to stop.".dimmed());
    println!("{}", "=========================================".blue());

    // Spawn external status poller (only if status_url is provided)
    if let Some(status_url) = args.status_url.clone() {
        let poller_client = client.clone();
        let mut poller_last_post_id: Option<String> = None;

        // First, sync with existing posts
        if let Ok(status) = fetch_status(&poller_client, &status_url).await {
            if let Some(posts) = status.get("posts").and_then(|p| p.as_array()) {
                if let Some(last) = posts.last() {
                    poller_last_post_id = last
                        .get("id")
                        .and_then(|id| id.as_str().map(|s| s.to_string()));
                }
            }
        }

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(2)).await;
                if let Ok(status) = fetch_status(&poller_client, &status_url).await {
                    if let Some(posts) = status.get("posts").and_then(|p| p.as_array()) {
                        let mut new_posts = Vec::new();
                        let mut found_last = poller_last_post_id.is_none();

                        for post in posts {
                            let id = post
                                .get("id")
                                .and_then(|id| id.as_str())
                                .unwrap_or_default();
                            if !found_last {
                                if Some(id.to_string()) == poller_last_post_id {
                                    found_last = true;
                                }
                                continue;
                            }
                            new_posts.push(post);
                        }

                        for post in new_posts {
                            let id = post
                                .get("id")
                                .and_then(|id| id.as_str())
                                .unwrap_or_default();
                            let content = post
                                .get("content")
                                .and_then(|c| c.as_str())
                                .unwrap_or_default();
                            let agent_id = post
                                .get("agent_id")
                                .and_then(|a| a.as_str())
                                .unwrap_or("agent");

                            println!(
                                "\n{} {}: {}",
                                ">>>".green().bold(),
                                agent_id.cyan(),
                                content
                            );
                            print!("{} ", "You:".yellow().bold());
                            io::stdout().flush().unwrap();

                            poller_last_post_id = Some(id.to_string());
                        }
                    }
                }
            }
        });
    }

    // Spawn direct message poller
    let direct_client = client.clone();
    let direct_url = args.connector_url.clone();
    let direct_secret = args.secret.clone();
    let direct_channel = args.channel_id.clone();

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(1)).await;
            match direct_client
                .get(format!("{}/connector/loopback/outbound", direct_url))
                .header("x-ccos-connector-secret", &direct_secret)
                .query(&[("channel_id", &direct_channel)])
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(messages) = resp.json::<Vec<OutboundRequest>>().await {
                        for msg in messages {
                            println!(
                                "\n{} {}: {}",
                                ">>> [DIRECT]".magenta().bold(),
                                "agent".cyan(),
                                msg.content
                            );
                            print!("{} ", "You:".yellow().bold());
                            io::stdout().flush().unwrap();
                        }
                    }
                }
                Err(_) => {}
            }
        }
    });

    loop {
        print!("{} ", "You:".yellow().bold());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            break;
        }

        let payload = json!({
            "channel_id": args.channel_id,
            "sender_id": args.user_id,
            "text": input,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        match client
            .post(format!("{}/connector/loopback/inbound", args.connector_url))
            .header("x-ccos-connector-secret", &args.secret)
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    println!("{} Error: {}", "!".red(), resp.status());
                }
            }
            Err(e) => {
                println!("{} Connection failed: {}", "!".red(), e);
            }
        }
    }

    Ok(())
}

async fn fetch_status(client: &Client, url: &str) -> anyhow::Result<serde_json::Value> {
    let resp = client.get(format!("{}/status", url)).send().await?;
    let json = resp.json().await?;
    Ok(json)
}

#[derive(Debug, serde::Deserialize)]
struct OutboundRequest {
    content: String,
}
