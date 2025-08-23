use crate::ccos::intent_storage::{InMemoryStorage, FileStorage, StorageConfig};
use crate::ccos::types::StorableIntent;
use crate::ccos::intent_graph::storage::Edge;
use tempfile::tempdir;

fn create_intent(goal: &str) -> StorableIntent {
    StorableIntent::new(goal.to_string())
}

#[tokio::test]
async fn test_file_storage_backup_and_restore_intent_graph() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("intent_storage.json");

    // Create file storage and add intents/edges
    let mut file_storage = FileStorage::new(path.clone()).await.unwrap();

    let intent1 = create_intent("Goal one");
    let id1 = intent1.intent_id.clone();
    file_storage.store_intent(intent1).await.unwrap();

    let intent2 = create_intent("Goal two");
    let id2 = intent2.intent_id.clone();
    file_storage.store_intent(intent2).await.unwrap();

    // Add edge between intents
    let edge = Edge::new(id1.clone(), id2.clone(), crate::ccos::intent_graph::storage::EdgeType::DependsOn);
    file_storage.store_edge(&edge).await.unwrap();

    // Backup to explicit path
    let backup_path = temp.path().join("backup.json");
    file_storage.backup(&backup_path).await.unwrap();
    assert!(backup_path.exists());

    // Create new in-memory storage and restore
    let mut new_storage = InMemoryStorage::new();
    new_storage.restore(&backup_path).await.unwrap();

    // Verify intents exist
    let restored1 = new_storage.get_intent(&id1).await.unwrap();
    assert!(restored1.is_some());
    let restored2 = new_storage.get_intent(&id2).await.unwrap();
    assert!(restored2.is_some());

    // Verify edge exists
    let edges = new_storage.get_edges().await.unwrap();
    assert!(edges.contains(&edge));
}
