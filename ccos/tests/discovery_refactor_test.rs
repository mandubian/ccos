use ccos::approval::{storage_file::FileApprovalStorage, UnifiedApprovalQueue};
use std::sync::Arc;

#[tokio::test]
async fn test_discovery_refactor_server_rtfs_parsing() {
    println!("🔍 Testing Discovery Refactor");

    // 1. Setup temporary directory
    let temp_dir = tempfile::tempdir().unwrap();
    let pending_dir = temp_dir.path().join("pending").join("test-server-1");
    std::fs::create_dir_all(&pending_dir).unwrap();
    println!("📂 Using temporary storage: {:?}", temp_dir.path());

    // 2. Create mock server.rtfs
    let server_rtfs = r#"(server
    :source {
        :type "OpenApi"
        :spec_url "https://api.example.com/openapi.json"
    }
    :server_info {
        :name "TestServer"
        :description "A test server"
        :endpoint "https://api.example.com"
        :auth_env_var nil
    }
    :api_info {
        :base_url "https://api.example.com"
        :endpoints_count 2
    }
    :capability_files [
        "openapi/api_v1/users.rtfs"
        "openapi/api_v1/posts.rtfs"
    ]
)"#;

    let rtfs_path = pending_dir.join("server.rtfs");
    std::fs::write(&rtfs_path, server_rtfs).unwrap();
    println!("✅ Created mock server.rtfs at {:?}", pending_dir);

    // 3. Initialize FileApprovalStorage and Queue
    let storage = FileApprovalStorage::new(temp_dir.path().to_path_buf()).unwrap();
    let queue = UnifiedApprovalQueue::new(Arc::new(storage));

    // 4. Verify Loading
    println!("🔍 Verifying request loading...");
    let requests = queue.list_pending().await.unwrap();

    assert_eq!(requests.len(), 1, "Should have 1 pending request");
    let req = &requests[0];

    // Verify ServerInfo and Category
    if let ccos::approval::types::ApprovalCategory::ServerDiscovery {
        server_info,
        source,
        capability_files,
        ..
    } = &req.category
    {
        println!("  Verifying ServerInfo...");
        assert_eq!(server_info.name, "TestServer");
        assert_eq!(server_info.endpoint, "https://api.example.com");

        println!("  Verifying DiscoverySource...");
        assert_eq!(
            source.name(),
            "openapi:https://api.example.com/openapi.json"
        );

        println!("  Verifying Capability Files...");
        let files = capability_files
            .as_ref()
            .expect("Capability files should be present");
        assert_eq!(files.len(), 2, "Should extract 2 capability files");
        assert!(files.contains(&"openapi/api_v1/users.rtfs".to_string()));
    } else {
        panic!("Request category should be ServerDiscovery");
    }

    println!("✅ Loading verification successful!");
}
