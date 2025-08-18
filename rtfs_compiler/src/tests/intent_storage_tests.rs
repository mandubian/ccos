#[cfg(test)]
mod intent_storage_tests {
    use crate::ccos::intent_storage::*;
    use crate::ccos::types::{StorableIntent, IntentStatus};
    use tempfile::tempdir;

    async fn create_test_intent(goal: &str) -> StorableIntent {
        StorableIntent::new(goal.to_string())
    }

    #[tokio::test]
    async fn test_in_memory_storage_basic() {
        let mut storage = InMemoryStorage::new();
        let intent = create_test_intent("Test in-memory storage").await;
        let intent_id = intent.intent_id.clone();

        // Test store
        let stored_id = storage.store_intent(intent.clone()).await.unwrap();
        assert_eq!(stored_id, intent_id);

        // Test retrieve
        let retrieved = storage.get_intent(&intent_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().goal, "Test in-memory storage");

        // Test update
        let mut updated_intent = intent.clone();
        updated_intent.status = IntentStatus::Completed;
        storage.update_intent(updated_intent).await.unwrap();

        let retrieved_updated = storage.get_intent(&intent_id).await.unwrap().unwrap();
        assert_eq!(retrieved_updated.status, IntentStatus::Completed);
    }

    #[tokio::test]
    async fn test_file_storage_persistence() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("test_storage.json");

        // Create file storage and store intent
        let mut storage = FileStorage::new(storage_path.clone()).await.unwrap();
        let intent = create_test_intent("File persistence test").await;
        let intent_id = intent.intent_id.clone();
        
        storage.store_intent(intent.clone()).await.unwrap();

        // Create new storage instance and verify persistence
        let new_storage = FileStorage::new(storage_path).await.unwrap();
        let retrieved = new_storage.get_intent(&intent_id).await.unwrap();
        
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().goal, "File persistence test");
    }

    #[tokio::test]
    async fn test_storage_factory() {
        // Test in-memory creation
        let in_memory_storage = StorageFactory::create(StorageConfig::InMemory).await;
        assert!(in_memory_storage.health_check().await.is_ok());

        // Test file storage creation
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("factory_test.json");
        let file_config = StorageConfig::File { path: file_path };
        let file_storage = StorageFactory::create(file_config).await;
        assert!(file_storage.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_intent_filtering() {
        let mut storage = InMemoryStorage::new();
        
        let mut intent1 = create_test_intent("Active task").await;
        intent1.status = IntentStatus::Active;
        
        let mut intent2 = create_test_intent("Completed task").await;
        intent2.status = IntentStatus::Completed;

        storage.store_intent(intent1).await.unwrap();
        storage.store_intent(intent2).await.unwrap();

        // Filter by status
        let active_filter = IntentFilter {
            status: Some(IntentStatus::Active),
            ..Default::default()
        };
        let active_intents = storage.list_intents(active_filter).await.unwrap();
        assert_eq!(active_intents.len(), 1);
        assert_eq!(active_intents[0].goal, "Active task");

        // Filter by goal content
        let goal_filter = IntentFilter {
            goal_contains: Some("Completed".to_string()),
            ..Default::default()
        };
        let matching_intents = storage.list_intents(goal_filter).await.unwrap();
        assert_eq!(matching_intents.len(), 1);
        assert_eq!(matching_intents[0].goal, "Completed task");
    }
}

#[cfg(test)]
mod intent_graph_update_integration {
    use crate::ccos::intent_graph::IntentGraph;
    use crate::ccos::types::{StorableIntent, ExecutionResult, IntentStatus};
    use crate::runtime::values::Value;

    #[test]
    fn orchestrator_style_update_sets_status() {
        let mut graph = IntentGraph::new().expect("Failed to create IntentGraph");

        // Store intent
        let intent = StorableIntent::new("Integration test goal".to_string());
        let id = intent.intent_id.clone();
        graph.store_intent(intent.clone()).expect("store_intent failed");

        // Update to success
        let success = ExecutionResult { success: true, value: Value::String("ok".to_string()), metadata: Default::default() };
        graph.update_intent(graph.get_intent(&id).unwrap(), &success).expect("update_intent failed");
        let got = graph.get_intent(&id).expect("get_intent failed");
        assert_eq!(got.status, IntentStatus::Completed);

        // New intent -> fail
        let intent2 = StorableIntent::new("Integration test goal 2".to_string());
        let id2 = intent2.intent_id.clone();
        graph.store_intent(intent2.clone()).expect("store_intent failed");
        let fail = ExecutionResult { success: false, value: Value::String("err".to_string()), metadata: Default::default() };
        graph.update_intent(graph.get_intent(&id2).unwrap(), &fail).expect("update_intent failed");
        let got2 = graph.get_intent(&id2).expect("get_intent failed");
        assert_eq!(got2.status, IntentStatus::Failed);
    }
}