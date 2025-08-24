//! Persistent Storage Backend for Intent Graph
//!
//! This module provides a flexible storage abstraction for the Intent Graph,
//! supporting multiple backends with graceful fallback to in-memory storage.

use super::intent_graph::Edge;
use super::types::{IntentId, IntentStatus, StorableIntent};
use crate::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for storage backend selection
#[derive(Debug, Clone)]
pub enum StorageConfig {
    /// In-memory storage (data lost on shutdown)
    InMemory,
    /// File-based storage with specified path
    File { path: PathBuf },
}

/// Storage-safe version of Value that excludes non-serializable types
/// This version can be safely stored and retrieved from async storage backends
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageValue {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Vector(Vec<StorageValue>),
    Map(HashMap<String, StorageValue>),
}

impl StorageValue {
    /// Convert from runtime Value to storage Value
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Nil => StorageValue::Null,
            Value::Boolean(b) => StorageValue::Boolean(*b),
            Value::Integer(i) => StorageValue::Number(*i as f64),
            Value::Float(f) => StorageValue::Number(*f),
            Value::String(s) => StorageValue::String(s.clone()),
            Value::Vector(v) => StorageValue::Vector(v.iter().map(StorageValue::from_value).collect()),
            Value::Map(m) => StorageValue::Map(m.iter().map(|(k, v)| (format!("{:?}", k), StorageValue::from_value(v))).collect()),
            // Skip non-serializable types
            Value::Function(_) => StorageValue::String("<<function>>".to_string()),
            Value::FunctionPlaceholder(_) => StorageValue::String("<<function_placeholder>>".to_string()),
            Value::Atom(_) => StorageValue::String("<<atom>>".to_string()),
            // Handle other Value variants
            Value::Timestamp(t) => StorageValue::String(format!("timestamp:{}", t)),
            Value::Uuid(u) => StorageValue::String(format!("uuid:{}", u)),
            Value::ResourceHandle(rh) => StorageValue::String(format!("resource:{}", rh)),
            Value::Symbol(s) => StorageValue::String(format!("symbol:{:?}", s)),
            Value::Keyword(k) => StorageValue::String(format!("keyword:{:?}", k)),
            Value::List(l) => StorageValue::Vector(l.iter().map(StorageValue::from_value).collect()),
            Value::Error(e) => StorageValue::String(format!("error:{}", e.message)),
        }
    }
}

/// Trait defining the storage interface for Intent Graph persistence
#[async_trait::async_trait]
pub trait IntentStorage {
    /// Persist a new intent
    async fn store_intent(&mut self, intent: StorableIntent) -> Result<IntentId, StorageError>;

    /// Retrieve an intent by ID
    async fn get_intent(&self, id: &IntentId) -> Result<Option<StorableIntent>, StorageError>;

    /// Update an existing intent
    async fn update_intent(&mut self, intent: StorableIntent) -> Result<(), StorageError>;

    /// Delete an intent by ID
    async fn delete_intent(&mut self, id: &IntentId) -> Result<(), StorageError>;

    /// List intents matching the given filter
    async fn list_intents(&self, filter: IntentFilter) -> Result<Vec<StorableIntent>, StorageError>;

    /// Store an edge relationship
    async fn store_edge(&mut self, edge: &Edge) -> Result<(), StorageError>;

    /// Get all edges
    async fn get_edges(&self) -> Result<Vec<Edge>, StorageError>;

    /// Get edges for a specific intent
    async fn get_edges_for_intent(&self, intent_id: &IntentId) -> Result<Vec<Edge>, StorageError>;

    /// Delete an edge
    async fn delete_edge(&mut self, edge: &Edge) -> Result<(), StorageError>;

    /// Create a backup of all data
    async fn backup(&self, path: &Path) -> Result<(), StorageError>;

    /// Restore data from a backup
    async fn restore(&mut self, path: &Path) -> Result<(), StorageError>;
    
    /// Check if storage backend is healthy and accessible
    async fn health_check(&self) -> Result<(), StorageError>;
}

/// Filter criteria for listing intents
#[derive(Debug, Clone)]
pub struct IntentFilter {
    pub status: Option<IntentStatus>,
    pub name_contains: Option<String>,
    pub goal_contains: Option<String>,
    pub priority_min: Option<u32>,
    pub priority_max: Option<u32>,
}

impl Default for IntentFilter {
    fn default() -> Self {
        Self {
            status: None,
            name_contains: None,
            goal_contains: None,
            priority_min: None,
            priority_max: None,
        }
    }
}

/// Storage errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Intent not found: {0}")]
    NotFound(IntentId),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    
    #[error("Storage error: {0}")]
    Storage(String),
}

/// In-memory storage implementation
pub struct InMemoryStorage {
    intents: Arc<RwLock<HashMap<IntentId, StorableIntent>>>,
    edges: Arc<RwLock<Vec<Edge>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            intents: Arc::new(RwLock::new(HashMap::new())),
            edges: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl IntentStorage for InMemoryStorage {
    async fn store_intent(&mut self, intent: StorableIntent) -> Result<IntentId, StorageError> {
        let intent_id = intent.intent_id.clone();
        self.intents.write().await.insert(intent_id.clone(), intent);
        Ok(intent_id)
    }

    async fn get_intent(&self, id: &IntentId) -> Result<Option<StorableIntent>, StorageError> {
        let intents = self.intents.read().await;
        Ok(intents.get(id).cloned())
    }

    async fn update_intent(&mut self, intent: StorableIntent) -> Result<(), StorageError> {
        let mut intents = self.intents.write().await;
        if intents.contains_key(&intent.intent_id) {
            intents.insert(intent.intent_id.clone(), intent);
            Ok(())
        } else {
            Err(StorageError::NotFound(intent.intent_id.clone()))
        }
    }

    async fn delete_intent(&mut self, id: &IntentId) -> Result<(), StorageError> {
        let mut intents = self.intents.write().await;
        if intents.remove(id).is_some() {
            Ok(())
        } else {
            Err(StorageError::NotFound(id.clone()))
        }
    }

    async fn list_intents(&self, filter: IntentFilter) -> Result<Vec<StorableIntent>, StorageError> {
        let intents = self.intents.read().await;
        let results: Vec<StorableIntent> = intents
            .values()
            .filter(|intent| {
                // Apply filters
                if let Some(status) = &filter.status {
                    if intent.status != *status {
                        return false;
                    }
                }
                
                if let Some(name_contains) = &filter.name_contains {
                    if !intent.name.as_ref().map_or(false, |n| n.contains(name_contains)) {
                        return false;
                    }
                }
                
                if let Some(goal_contains) = &filter.goal_contains {
                    if !intent.goal.contains(goal_contains) {
                        return false;
                    }
                }
                
                if let Some(priority_min) = filter.priority_min {
                    if intent.priority < priority_min {
                        return false;
                    }
                }
                
                if let Some(priority_max) = filter.priority_max {
                    if intent.priority > priority_max {
                        return false;
                    }
                }
                
                true
            })
            .cloned()
            .collect();
        
        Ok(results)
    }

    async fn store_edge(&mut self, edge: &Edge) -> Result<(), StorageError> {
        let mut edges = self.edges.write().await;
        edges.push(edge.clone());
        Ok(())
    }

    async fn get_edges(&self) -> Result<Vec<Edge>, StorageError> {
        let edges = self.edges.read().await;
        Ok(edges.clone())
    }

    async fn get_edges_for_intent(&self, intent_id: &IntentId) -> Result<Vec<Edge>, StorageError> {
        let edges = self.edges.read().await;
        Ok(edges.iter()
            .filter(|edge| &edge.from == intent_id || &edge.to == intent_id)
            .cloned()
            .collect())
    }

    async fn delete_edge(&mut self, edge: &Edge) -> Result<(), StorageError> {
        let mut edges = self.edges.write().await;
        if let Some(pos) = edges.iter().position(|e| e == edge) {
            edges.remove(pos);
            Ok(())
        } else {
            Err(StorageError::Storage("Edge not found".to_string()))
        }
    }

    async fn backup(&self, path: &Path) -> Result<(), StorageError> {
        let intents = self.intents.read().await;
        let edges = self.edges.read().await;

        let backup_data = StorageBackupData::new(intents.clone(), edges.clone());

        let json = serde_json::to_string_pretty(&backup_data)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        // Atomic write
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut tmp = path.to_path_buf();
        let tmp_name = format!("{}.tmp-{}", path.file_name().and_then(|s| s.to_str()).unwrap_or("backup.json"), std::process::id());
        tmp.set_file_name(tmp_name);
        if let Some(dir) = path.parent() { tmp = dir.join(tmp.file_name().unwrap()); }
        {
            let mut f = fs::File::create(&tmp)?;
            use std::io::Write as _;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    async fn restore(&mut self, path: &Path) -> Result<(), StorageError> {
        let content = fs::read_to_string(path)?;
    let backup_data: StorageBackupData = serde_json::from_str(&content)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let mut intents = self.intents.write().await;
        let mut edges = self.edges.write().await;

        *intents = backup_data.intents;
        *edges = backup_data.edges;

        Ok(())
    }

    async fn health_check(&self) -> Result<(), StorageError> {
        // For in-memory storage, just check if we can access the data structures
        let _intents = self.intents.read().await;
        let _edges = self.edges.read().await;
        Ok(())
    }
}

/// File-based storage implementation
pub struct FileStorage {
    in_memory: InMemoryStorage,
    file_path: PathBuf,
}

impl FileStorage {
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let mut storage = Self {
            in_memory: InMemoryStorage::new(),
            file_path: path.clone(),
        };
        
        // Try to load existing data
        if path.exists() {
            storage.load_from_file().await?;
        }
        
        Ok(storage)
    }
    
    async fn save_to_file(&self) -> Result<(), StorageError> {
        let intents = self.in_memory.intents.read().await;
        let edges = self.in_memory.edges.read().await;

        let backup_data = StorageBackupData::new(intents.clone(), edges.clone());

        let json = serde_json::to_string_pretty(&backup_data)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        // Atomic write: write to temp file in same dir then rename
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut tmp = self.file_path.clone();
        let tmp_name = format!("{}.tmp-{}", self.file_path.file_name().and_then(|s| s.to_str()).unwrap_or("storage.json"), std::process::id());
        tmp.set_file_name(tmp_name);
        if let Some(dir) = self.file_path.parent() { tmp = dir.join(tmp.file_name().unwrap()); }
        {
            let mut f = fs::File::create(&tmp)?;
            use std::io::Write as _;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &self.file_path)?;
        Ok(())
    }
    
    async fn load_from_file(&mut self) -> Result<(), StorageError> {
        let content = tokio::fs::read_to_string(&self.file_path).await?;
        let backup_data: StorageBackupData = serde_json::from_str(&content)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let mut intents = self.in_memory.intents.write().await;
        let mut edges = self.in_memory.edges.write().await;

        *intents = backup_data.intents;
        *edges = backup_data.edges;

        Ok(())
    }
}

#[async_trait::async_trait]
impl IntentStorage for FileStorage {
    async fn store_intent(&mut self, intent: StorableIntent) -> Result<IntentId, StorageError> {
        let result = self.in_memory.store_intent(intent).await?;
        self.save_to_file().await?;
        Ok(result)
    }

    async fn get_intent(&self, id: &IntentId) -> Result<Option<StorableIntent>, StorageError> {
        self.in_memory.get_intent(id).await
    }

    async fn update_intent(&mut self, intent: StorableIntent) -> Result<(), StorageError> {
        self.in_memory.update_intent(intent).await?;
        self.save_to_file().await?;
        Ok(())
    }

    async fn delete_intent(&mut self, id: &IntentId) -> Result<(), StorageError> {
        self.in_memory.delete_intent(id).await?;
        self.save_to_file().await?;
        Ok(())
    }

    async fn list_intents(&self, filter: IntentFilter) -> Result<Vec<StorableIntent>, StorageError> {
        self.in_memory.list_intents(filter).await
    }

    async fn store_edge(&mut self, edge: &Edge) -> Result<(), StorageError> {
        self.in_memory.store_edge(edge).await?;
        self.save_to_file().await?;
        Ok(())
    }

    async fn get_edges(&self) -> Result<Vec<Edge>, StorageError> {
        self.in_memory.get_edges().await
    }

    async fn get_edges_for_intent(&self, intent_id: &IntentId) -> Result<Vec<Edge>, StorageError> {
        self.in_memory.get_edges_for_intent(intent_id).await
    }

    async fn delete_edge(&mut self, edge: &Edge) -> Result<(), StorageError> {
        self.in_memory.delete_edge(edge).await?;
        self.save_to_file().await?;
        Ok(())
    }

    async fn backup(&self, path: &Path) -> Result<(), StorageError> {
        self.in_memory.backup(path).await
    }

    async fn restore(&mut self, path: &Path) -> Result<(), StorageError> {
        self.in_memory.restore(path).await?;
        self.save_to_file().await?;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), StorageError> {
        // Check that we can access the file and it's valid
        if self.file_path.exists() {
            // Try to read the file to ensure it's valid JSON
            let _content = fs::read_to_string(&self.file_path)?;
        }
        // Also check underlying in-memory storage
        self.in_memory.health_check().await
    }
}

/// Backup data structure for serialization
#[derive(Debug, Serialize, Deserialize)]
struct StorageBackupData {
    intents: HashMap<IntentId, StorableIntent>,
    edges: Vec<Edge>,
    version: String,
    timestamp: u64,
    #[serde(default)]
    manifest: Option<BackupManifest>,
    #[serde(default)]
    rtfs: Option<String>,
}

/// Optional manifest metadata embedded in backups
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct BackupManifest {
    /// Identifier for the producer of the backup
    created_by: String,
    /// Optional free-form source identifier (e.g., file path, node id)
    #[serde(default)]
    source: Option<String>,
    /// Optional note about the backup purpose
    #[serde(default)]
    note: Option<String>,
}

impl StorageBackupData {
    fn new(intents: HashMap<IntentId, StorableIntent>, edges: Vec<Edge>) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let rtfs = Some(Self::render_rtfs(&intents, &edges));
        let manifest = Some(BackupManifest {
            created_by: "rtfs_compiler".to_string(),
            source: None,
            note: Some("Hybrid JSON+RTFS backup".to_string()),
        });

        Self {
            intents,
            edges,
            version: "1.1".to_string(),
            timestamp,
            manifest,
            rtfs,
        }
    }

    /// Render a human-readable RTFS snapshot alongside the JSON backup
    fn render_rtfs(intents: &HashMap<IntentId, StorableIntent>, edges: &Vec<Edge>) -> String {
        fn esc(s: &str) -> String { s.replace('\\', "\\\\").replace('"', "\\\"") }

        let mut out = String::new();
        out.push_str(";; Intent Graph Snapshot (hybrid backup)\n");
        out.push_str("(intent-graph\n");
        out.push_str("  (intents\n");
        for (_id, intent) in intents.iter() {
            out.push_str(&format!(
                "    (intent {{:id \"{}\" :goal \"{}\" :status \"{:?}\" :priority {}{}{} }})\n",
                esc(&intent.intent_id),
                esc(&intent.goal),
                intent.status,
                intent.priority,
                match &intent.name { Some(n) => format!(" :name \"{}\"", esc(n)), None => String::new() },
                if !intent.rtfs_intent_source.is_empty() {
                    format!(" :rtfs-intent \"{}\"", esc(&intent.rtfs_intent_source))
                } else { String::new() }
            ));
        }
        out.push_str("  )\n");
        out.push_str("  (edges\n");
        for e in edges.iter() {
            out.push_str(&format!(
                "    (edge {{:from \"{}\" :to \"{}\" :type \"{:?}\"{} }})\n",
                esc(&e.from),
                esc(&e.to),
                e.edge_type,
                match e.weight { Some(w) => format!(" :weight {}", w), None => String::new() }
            ));
        }
        out.push_str("  )\n");
        out.push_str(")\n");
        out
    }
}

/// Storage factory for creating different storage backends
pub struct StorageFactory;

impl StorageFactory {
    /// Create storage backend based on configuration
    pub async fn create(config: StorageConfig) -> Box<dyn IntentStorage> {
        match config {
            StorageConfig::InMemory => Self::in_memory(),
            StorageConfig::File { path } => {
                match Self::file(path).await {
                    Ok(storage) => storage,
                    Err(_) => {
                        eprintln!("Note: Using in-memory storage for fallback strategy");
                        Self::with_fallback()
                    }
                }
            }
        }
    }

    /// Create an in-memory storage backend
    pub fn in_memory() -> Box<dyn IntentStorage> {
        Box::new(InMemoryStorage::new())
    }
    
    /// Create a file-based storage backend
    pub async fn file<P: AsRef<Path>>(path: P) -> Result<Box<dyn IntentStorage>, StorageError> {
        Ok(Box::new(FileStorage::new(path).await?))
    }
    
    /// Create storage with fallback strategy (starts as in-memory, can be upgraded later)
    pub fn with_fallback() -> Box<dyn IntentStorage> {
        // For now, just return in-memory since async construction in sync context is complex
        eprintln!("Note: Using in-memory storage for fallback strategy");
        Box::new(InMemoryStorage::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::types::{IntentStatus};
    use tempfile::tempdir;

    fn create_test_intent(goal: &str) -> StorableIntent {
        StorableIntent::new(goal.to_string())
    }

    #[tokio::test]
    async fn test_in_memory_storage() {
        let mut storage = InMemoryStorage::new();
        let intent = create_test_intent("Test goal");
        let intent_id = intent.intent_id.clone();

        // Store intent
        let stored_id = storage.store_intent(intent).await.unwrap();
        assert_eq!(stored_id, intent_id);

        // Retrieve intent
        let retrieved = storage.get_intent(&intent_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().goal, "Test goal");

        // List intents
        let all_intents = storage.list_intents(IntentFilter::default()).await.unwrap();
        assert_eq!(all_intents.len(), 1);
    }

    #[tokio::test]
    async fn test_file_storage() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("test_storage.json");

        let mut storage = FileStorage::new(storage_path.clone()).await.unwrap();
        let intent = create_test_intent("File storage test");
        let intent_id = intent.intent_id.clone();

        // Store intent
        storage.store_intent(intent).await.unwrap();

        // Verify file was created
        assert!(storage_path.exists());

        // Create new storage instance and verify data persists
        let storage2 = FileStorage::new(storage_path).await.unwrap();
        let retrieved = storage2.get_intent(&intent_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().goal, "File storage test");
    }

    #[tokio::test]
    async fn test_intent_filter() {
        let mut storage = InMemoryStorage::new();
        
        let mut intent1 = create_test_intent("Active task");
        intent1.status = IntentStatus::Active;
        
        let mut intent2 = create_test_intent("Completed task");
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

    #[tokio::test]
    async fn test_storage_factory_fallback() {
        // Test with invalid file path
        let invalid_config = StorageConfig::File {
            path: PathBuf::from("/invalid/path/that/does/not/exist/storage.json"),
        };

        let storage = StorageFactory::create(invalid_config).await;
        
        // Should fall back to in-memory storage
        assert!(storage.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_backup_restore() {
        let mut storage = InMemoryStorage::new();
        let intent = create_test_intent("Backup test");
        let intent_id = intent.intent_id.clone(); // Clone ID before moving intent
        storage.store_intent(intent).await.unwrap();

        let temp_dir = tempdir().unwrap();
        let backup_path = temp_dir.path().join("backup.json");

        // Backup
        storage.backup(&backup_path).await.unwrap();
        assert!(backup_path.exists());

    // Validate hybrid fields present
    let content = std::fs::read_to_string(&backup_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["version"], serde_json::json!("1.1"));
    assert!(v.get("manifest").is_some());
    assert_eq!(v["manifest"]["created_by"], serde_json::json!("rtfs_compiler"));
    assert!(v.get("rtfs").is_some());
    let rtfs_str = v["rtfs"].as_str().unwrap();
    assert!(rtfs_str.contains("(intent-graph"));

        // Create new storage and restore
        let mut new_storage = InMemoryStorage::new();
        new_storage.restore(&backup_path).await.unwrap();

        let restored = new_storage.get_intent(&intent_id).await.unwrap();
        assert!(restored.is_some());
        assert_eq!(restored.unwrap().goal, "Backup test");
    }
}