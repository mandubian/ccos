#[cfg(test)]
mod intent_storage_tests {
    use crate::ccos::intent_storage::*;
    use crate::ccos::types::{Intent, IntentStatus};
    use tempfile::tempdir;

    async fn create_test_intent(goal: &str) -> Intent {
        Intent::new(goal.to_string())
    }

    #[tokio::test]
    async fn test_in_memory_storage_basic() {
        let mut storage = InMemoryStorage::new();
        let intent = create_test_intent("Test in-memory storage").await;
        let intent_id = intent.intent_id.clone();

        // Test store
        let stored_id = storage.store_intent(&intent).await.unwrap();
        assert_eq!(stored_id, intent_id);

        // Test retrieve
        let retrieved = storage.get_intent(&intent_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().goal, "Test in-memory storage");

        // Test update
        let mut updated_intent = intent.clone();
        updated_intent.status = IntentStatus::Completed;
        storage.update_intent(&updated_intent).await.unwrap();

        let retrieved_updated = storage.get_intent(&intent_id).await.unwrap().unwrap();
        assert_eq!(retrieved_updated.status, IntentStatus::Completed);
    }

    #[tokio::test]
    async fn test_file_storage_persistence() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("test_storage.json");

        // Create file storage and store intent
        let mut storage = FileStorage::new(storage_path.clone()).unwrap();
        let intent = create_test_intent("File persistence test").await;
        let intent_id = intent.intent_id.clone();
        
        storage.store_intent(&intent).await.unwrap();

        // Create new storage instance and verify persistence
        let new_storage = FileStorage::new(storage_path).unwrap();
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

        storage.store_intent(&intent1).await.unwrap();
        storage.store_intent(&intent2).await.unwrap();

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