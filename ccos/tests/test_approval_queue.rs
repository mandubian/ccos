use ccos::discovery::approval_queue::{ApprovalQueue, PendingDiscovery, DiscoverySource, ServerInfo, RiskAssessment, RiskLevel};
use chrono::Utc;
use tempfile::tempdir;

#[test]
fn test_approval_workflow() {
    let dir = tempdir().unwrap();
    let queue = ApprovalQueue::new(dir.path());
    
    // Create pending
    let discovery = PendingDiscovery {
        id: "test-1".to_string(),
        source: DiscoverySource::Manual { user: "test".to_string() },
        server_info: ServerInfo {
            name: "test-server".to_string(),
            endpoint: "http://localhost".to_string(),
            description: None,
        },
        domain_match: vec![],
        risk_assessment: RiskAssessment {
            level: RiskLevel::Low,
            reasons: vec![],
        },
        requested_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::hours(1),
        requesting_goal: None,
    };
    
    // Add
    queue.add(discovery).unwrap();
    
    // List
    let pending = queue.list_pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "test-1");
    
    // Approve
    queue.approve("test-1", Some("Looks good".to_string())).unwrap();
    
    // Check pending empty
    let pending = queue.list_pending().unwrap();
    assert!(pending.is_empty());
    
    // Check approved
    let approved = queue.list_approved().unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].id, "test-1");
    assert_eq!(approved[0].approval_reason, Some("Looks good".to_string()));
}

#[test]
fn test_rejection_workflow() {
    let dir = tempdir().unwrap();
    let queue = ApprovalQueue::new(dir.path());
    
    // Create pending
    let discovery = PendingDiscovery {
        id: "test-2".to_string(),
        source: DiscoverySource::Manual { user: "test".to_string() },
        server_info: ServerInfo {
            name: "bad-server".to_string(),
            endpoint: "http://localhost".to_string(),
            description: None,
        },
        domain_match: vec![],
        risk_assessment: RiskAssessment {
            level: RiskLevel::High,
            reasons: vec![],
        },
        requested_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::hours(1),
        requesting_goal: None,
    };
    
    queue.add(discovery).unwrap();
    
    // Reject
    queue.reject("test-2", "Too risky".to_string()).unwrap();
    
    // Check pending empty
    let pending = queue.list_pending().unwrap();
    assert!(pending.is_empty());
    
    // Check rejected
    let rejected = queue.list_rejected().unwrap();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].id, "test-2");
    assert_eq!(rejected[0].rejection_reason, "Too risky");
}

