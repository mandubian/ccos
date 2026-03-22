use std::sync::{Arc, Mutex};

use autonoetic_gateway::execution::GatewayExecutionService;
use autonoetic_gateway::scheduler::run_scheduler_tick;
use autonoetic_gateway::scheduler::signal::Signal;
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::notification::{NotificationRecord, NotificationStatus, NotificationType};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn test_store_only_notification_delivery_is_ordered_and_marked_delivered(
) -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir)?;

    // Bind a local test server and use its assigned port in gateway config.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        port,
        ..GatewayConfig::default()
    };

    let store = Arc::new(autonoetic_gateway::scheduler::gateway_store::GatewayStore::open(
        &gateway_dir,
    )?);

    let received_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_ids_server = Arc::clone(&received_ids);

    let server = tokio::spawn(async move {
        // Expect 2 notification deliveries.
        for _ in 0..2 {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let (read_half, mut write_half) = socket.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;

            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(id) = v
                    .get("params")
                    .and_then(|p| p.get("metadata"))
                    .and_then(|m| m.get("approval_request_id"))
                    .and_then(|id| id.as_str())
                {
                    received_ids_server.lock().unwrap().push(id.to_string());
                }
            }

            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "test-response",
                "result": {"ok": true}
            })
            .to_string();
            let _ = write_half.write_all(response.as_bytes()).await;
            let _ = write_half.write_all(b"\n").await;
            let _ = write_half.flush().await;
        }
    });

    // Create two pending notifications directly in store (store-only path, no signal files).
    let mut n1 = NotificationRecord::new(
        "ntf-aaa11111".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::to_value(Signal::ApprovalResolved {
            request_id: "apr-0001aaaa".to_string(),
            agent_id: "coder.default".to_string(),
            status: "approved".to_string(),
            install_completed: false,
            message: "First approval".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })?,
    );
    n1.request_id = Some("apr-0001aaaa".to_string());
    n1.created_at = "2026-03-22T14:00:00Z".to_string();

    let mut n2 = NotificationRecord::new(
        "ntf-bbb22222".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::to_value(Signal::ApprovalResolved {
            request_id: "apr-0002bbbb".to_string(),
            agent_id: "coder.default".to_string(),
            status: "approved".to_string(),
            install_completed: false,
            message: "Second approval".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })?,
    );
    n2.request_id = Some("apr-0002bbbb".to_string());
    n2.created_at = "2026-03-22T14:00:01Z".to_string();

    store.create_notification_record(&n1)?;
    store.create_notification_record(&n2)?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone(), Some(store.clone())));
    run_scheduler_tick(execution).await?;

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;

    let delivered_1 = store.get_notification("ntf-aaa11111")?.unwrap();
    let delivered_2 = store.get_notification("ntf-bbb22222")?.unwrap();

    assert_eq!(delivered_1.status, NotificationStatus::Delivered);
    assert_eq!(delivered_2.status, NotificationStatus::Delivered);

    let seen = received_ids.lock().unwrap().clone();
    assert_eq!(seen, vec!["apr-0001aaaa".to_string(), "apr-0002bbbb".to_string()]);

    Ok(())
}

#[tokio::test]
async fn test_pending_notifications_accept_current_payloads_and_fail_invalid(
) -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir)?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        port,
        ..GatewayConfig::default()
    };

    let store = Arc::new(autonoetic_gateway::scheduler::gateway_store::GatewayStore::open(
        &gateway_dir,
    )?);

    let received_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_ids_server = Arc::clone(&received_ids);

    let server = tokio::spawn(async move {
        // Two valid notifications should be delivered; invalid payload should not be sent.
        for _ in 0..2 {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let (read_half, mut write_half) = socket.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;

            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(id) = v
                    .get("params")
                    .and_then(|p| p.get("metadata"))
                    .and_then(|m| m.get("approval_request_id"))
                    .and_then(|id| id.as_str())
                {
                    received_ids_server.lock().unwrap().push(id.to_string());
                }
            }

            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "test-response",
                "result": {"ok": true}
            })
            .to_string();
            let _ = write_half.write_all(response.as_bytes()).await;
            let _ = write_half.write_all(b"\n").await;
            let _ = write_half.flush().await;
        }
    });

    // 1) Current Signal payload (valid)
    let mut n_lower = NotificationRecord::new(
        "ntf-lower0001".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::to_value(Signal::ApprovalResolved {
            request_id: "apr-mix0001".to_string(),
            agent_id: "coder.default".to_string(),
            status: "approved".to_string(),
            install_completed: false,
            message: "current payload".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })?,
    );
    n_lower.request_id = Some("apr-mix0001".to_string());
    n_lower.created_at = "2026-03-22T15:00:00Z".to_string();

    // 2) Another current Signal payload (valid)
    let mut n_modern = NotificationRecord::new(
        "ntf-modern0002".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::to_value(Signal::ApprovalResolved {
            request_id: "apr-mix0002".to_string(),
            agent_id: "coder.default".to_string(),
            status: "approved".to_string(),
            install_completed: false,
            message: "second current payload".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })?,
    );
    n_modern.request_id = Some("apr-mix0002".to_string());
    n_modern.created_at = "2026-03-22T15:00:01Z".to_string();

    // 3) Invalid payload should be marked failed (never delivered)
    let mut n_invalid = NotificationRecord::new(
        "ntf-invalid0003".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::json!({"foo": "bar"}),
    );
    n_invalid.request_id = Some("apr-mix0003".to_string());
    n_invalid.created_at = "2026-03-22T15:00:02Z".to_string();

    store.create_notification_record(&n_lower)?;
    store.create_notification_record(&n_modern)?;
    store.create_notification_record(&n_invalid)?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone(), Some(store.clone())));
    run_scheduler_tick(execution).await?;

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;

    let delivered_lower = store.get_notification("ntf-lower0001")?.unwrap();
    let delivered_modern = store.get_notification("ntf-modern0002")?.unwrap();
    let failed_invalid = store.get_notification("ntf-invalid0003")?.unwrap();

    assert_eq!(delivered_lower.status, NotificationStatus::Delivered);
    assert_eq!(delivered_modern.status, NotificationStatus::Delivered);
    assert_eq!(failed_invalid.status, NotificationStatus::Failed);
    assert_eq!(failed_invalid.attempt_count, 1);

    let seen = received_ids.lock().unwrap().clone();
    assert_eq!(seen, vec!["apr-mix0001".to_string(), "apr-mix0002".to_string()]);

    Ok(())
}

#[tokio::test]
async fn test_pending_notifications_with_same_timestamp_are_ordered_by_notification_id(
) -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir)?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        port,
        ..GatewayConfig::default()
    };

    let store = Arc::new(autonoetic_gateway::scheduler::gateway_store::GatewayStore::open(
        &gateway_dir,
    )?);

    let received_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_ids_server = Arc::clone(&received_ids);

    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let (read_half, mut write_half) = socket.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;

            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(id) = v
                    .get("params")
                    .and_then(|p| p.get("metadata"))
                    .and_then(|m| m.get("approval_request_id"))
                    .and_then(|id| id.as_str())
                {
                    received_ids_server.lock().unwrap().push(id.to_string());
                }
            }

            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "test-response",
                "result": {"ok": true}
            })
            .to_string();
            let _ = write_half.write_all(response.as_bytes()).await;
            let _ = write_half.write_all(b"\n").await;
            let _ = write_half.flush().await;
        }
    });

    let same_created_at = "2026-03-22T16:00:00Z".to_string();

    let mut n_z = NotificationRecord::new(
        "ntf-zzz99999".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::to_value(Signal::ApprovalResolved {
            request_id: "apr-order-z".to_string(),
            agent_id: "coder.default".to_string(),
            status: "approved".to_string(),
            install_completed: false,
            message: "z".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })?,
    );
    n_z.request_id = Some("apr-order-z".to_string());
    n_z.created_at = same_created_at.clone();

    let mut n_a = NotificationRecord::new(
        "ntf-aaa00000".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::to_value(Signal::ApprovalResolved {
            request_id: "apr-order-a".to_string(),
            agent_id: "coder.default".to_string(),
            status: "approved".to_string(),
            install_completed: false,
            message: "a".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })?,
    );
    n_a.request_id = Some("apr-order-a".to_string());
    n_a.created_at = same_created_at;

    // Insert in reverse order to ensure DB ordering decides delivery order.
    store.create_notification_record(&n_z)?;
    store.create_notification_record(&n_a)?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone(), Some(store.clone())));
    run_scheduler_tick(execution).await?;

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;

    let seen = received_ids.lock().unwrap().clone();
    assert_eq!(seen, vec!["apr-order-a".to_string(), "apr-order-z".to_string()]);

    Ok(())
}

#[tokio::test]
async fn test_pending_notification_delivery_retries_then_marks_failed() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir)?;

    // Reserve then drop a local port so delivery attempts hit connection-refused.
    let reserved = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = reserved.local_addr()?.port();
    drop(reserved);

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        port,
        ..GatewayConfig::default()
    };

    let store = Arc::new(autonoetic_gateway::scheduler::gateway_store::GatewayStore::open(
        &gateway_dir,
    )?);

    let mut n = NotificationRecord::new(
        "ntf-retry0001".to_string(),
        NotificationType::ApprovalResolved,
        "demo-session/coder.default-1".to_string(),
        serde_json::to_value(Signal::ApprovalResolved {
            request_id: "apr-retry0001".to_string(),
            agent_id: "coder.default".to_string(),
            status: "approved".to_string(),
            install_completed: false,
            message: "retry test".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })?,
    );
    n.request_id = Some("apr-retry0001".to_string());
    n.created_at = "2026-03-22T17:00:00Z".to_string();
    store.create_notification_record(&n)?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone(), Some(store.clone())));

    run_scheduler_tick(execution.clone()).await?;
    let after_1 = store.get_notification("ntf-retry0001")?.unwrap();
    assert_eq!(after_1.status, NotificationStatus::Pending);
    assert_eq!(after_1.attempt_count, 1);

    run_scheduler_tick(execution.clone()).await?;
    let after_2 = store.get_notification("ntf-retry0001")?.unwrap();
    assert_eq!(after_2.status, NotificationStatus::Pending);
    assert_eq!(after_2.attempt_count, 2);

    run_scheduler_tick(execution).await?;
    let after_3 = store.get_notification("ntf-retry0001")?.unwrap();
    assert_eq!(after_3.status, NotificationStatus::Failed);
    assert_eq!(after_3.attempt_count, 3);

    Ok(())
}
