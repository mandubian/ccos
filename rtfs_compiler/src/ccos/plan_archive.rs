use super::archivable_types::ArchivablePlan;
use super::storage::{ContentAddressableArchive, InMemoryArchive, IndexedArchive};
use super::storage_backends::file_archive::FileArchive;
use super::types::{IntentId, Plan, PlanId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
/// Storage backend for PlanArchive
pub enum PlanArchiveStorage {
    InMemory(InMemoryArchive<ArchivablePlan>),
    File(IndexedArchive<ArchivablePlan, FileArchive>),
}

/// Domain-specific Plan archive with indexing and retrieval capabilities
pub struct PlanArchive {
    storage: PlanArchiveStorage,
    plan_id_index: Arc<Mutex<HashMap<PlanId, String>>>,
    intent_id_index: Arc<Mutex<HashMap<IntentId, Vec<String>>>>,
}

impl PlanArchive {
    pub fn rehydrate(&mut self) -> Result<(), String> {
        // Try loading indices from storage sidecar if available. If load succeeds, use them.
        // If loading fails (parse/IO error) or sidecars are missing we'll fall back to scanning stored plans
        // and rebuild indices, then persist repaired sidecars.
        if let PlanArchiveStorage::File(f) = &self.storage {
            match f.try_load_indices() {
                Ok(true) => {
                    // Populate local mutexes from the wrapper's clones
                    let plan_map = f.plan_index_clone();
                    let intent_map = f.intent_index_clone();
                    {
                        let mut p = self
                            .plan_id_index
                            .lock()
                            .map_err(|_| "plan index lock poisoned".to_string())?;
                        *p = plan_map
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                    }
                    {
                        let mut i = self
                            .intent_id_index
                            .lock()
                            .map_err(|_| "intent index lock poisoned".to_string())?;
                        *i = intent_map
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                    }
                    return Ok(());
                }
                Ok(false) => {
                    // sidecars not present; fallthrough to scanning
                }
                Err(e) => {
                    // Could not read/parse sidecars; log and fall back to scanning and repair
                    eprintln!(
                        "⚠️ Failed to load sidecar indices (will rebuild by scanning): {}",
                        e
                    );
                }
            }
        }

        let hashes: Vec<String> = match &self.storage {
            PlanArchiveStorage::File(f) => {
                <IndexedArchive<ArchivablePlan, FileArchive> as ContentAddressableArchive<
                    ArchivablePlan,
                >>::list_hashes(f)
            }
            PlanArchiveStorage::InMemory(_) => return Ok(()),
        };
        if hashes.is_empty() {
            return Ok(());
        }
        for h in hashes {
            if let Some(plan) = self.retrieve_plan(&h)? {
                {
                    let mut p = self
                        .plan_id_index
                        .lock()
                        .map_err(|_| "plan index lock poisoned".to_string())?;
                    p.insert(plan.plan_id.clone(), h.clone());
                    // If file-backed, update wrapper indices so we can persist repaired sidecars
                    if let PlanArchiveStorage::File(f) = &self.storage {
                        let _ = f.insert_plan_mapping(plan.plan_id.clone(), h.clone());
                    }
                }
                {
                    let mut i = self
                        .intent_id_index
                        .lock()
                        .map_err(|_| "intent index lock poisoned".to_string())?;
                    for iid in &plan.intent_ids {
                        i.entry(iid.clone())
                            .or_insert_with(Vec::new)
                            .push(h.clone());
                        if let PlanArchiveStorage::File(f) = &self.storage {
                            let _ = f.add_intent_mapping(iid.clone(), h.clone());
                        }
                    }
                }
            }
        }
        // Persist via storage backend if available
        if let PlanArchiveStorage::File(f) = &self.storage {
            if let Err(e) = f.try_persist_indices() {
                eprintln!("⚠️ Failed to persist plan indices via storage: {}", e);
            }
        }
        Ok(())
    }
    pub fn new() -> Self {
        Self {
            storage: PlanArchiveStorage::InMemory(InMemoryArchive::new()),
            plan_id_index: Arc::new(Mutex::new(HashMap::new())),
            intent_id_index: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_file_storage(path: PathBuf) -> Result<Self, String> {
        let file_archive = FileArchive::new(path.clone()).map_err(|e| e.to_string())?;
        let indexed = IndexedArchive::new(file_archive);
        let mut this = Self {
            storage: PlanArchiveStorage::File(indexed),
            plan_id_index: Arc::new(Mutex::new(HashMap::new())),
            intent_id_index: Arc::new(Mutex::new(HashMap::new())),
        };
        if let Err(e) = this.rehydrate() {
            eprintln!("⚠️ PlanArchive rehydrate failed: {}", e);
        }
        Ok(this)
    }

    /// Store a plan using the appropriate storage backend
    fn store_plan(&self, plan: &ArchivablePlan) -> Result<String, String> {
        match &self.storage {
            PlanArchiveStorage::InMemory(archive) => archive.store(plan.clone()),
            PlanArchiveStorage::File(archive) => {
                <IndexedArchive<ArchivablePlan, FileArchive> as ContentAddressableArchive<
                    ArchivablePlan,
                >>::store(archive, plan.clone())
            }
        }
    }

    /// Retrieve a plan using the appropriate storage backend
    fn retrieve_plan(&self, hash: &str) -> Result<Option<ArchivablePlan>, String> {
        match &self.storage {
            PlanArchiveStorage::InMemory(archive) => archive.retrieve(hash),
            PlanArchiveStorage::File(archive) => {
                <IndexedArchive<ArchivablePlan, FileArchive> as ContentAddressableArchive<
                    ArchivablePlan,
                >>::retrieve(archive, hash)
            }
        }
    }

    /// Archive a plan and update indices
    pub fn archive_plan(&self, plan: &Plan) -> Result<String, String> {
        let archivable_plan = ArchivablePlan::from(plan);
        let hash = self.store_plan(&archivable_plan)?;

        // Update plan_id index
        {
            let mut plan_index = self.plan_id_index.lock().unwrap();
            plan_index.insert(plan.plan_id.clone(), hash.clone());
            // If file-backed, also update the storage wrapper's in-memory indices
            if let PlanArchiveStorage::File(f) = &self.storage {
                let _ = f.insert_plan_mapping(plan.plan_id.clone(), hash.clone());
            }
        }

        // Update intent_id index for all intent IDs associated with this plan
        {
            let mut intent_index = self.intent_id_index.lock().unwrap();
            for intent_id in &plan.intent_ids {
                intent_index
                    .entry(intent_id.clone())
                    .or_insert_with(Vec::new)
                    .push(hash.clone());
                if let PlanArchiveStorage::File(f) = &self.storage {
                    let _ = f.add_intent_mapping(intent_id.clone(), hash.clone());
                }
            }
        }

        // Persist indices via storage backend if possible
        if let PlanArchiveStorage::File(f) = &self.storage {
            if let Err(e) = f.try_persist_indices() {
                eprintln!("⚠️ Failed to persist plan indices via storage: {}", e);
            }
        }

        Ok(hash)
    }

    /// Retrieve a plan by its ID
    pub fn get_plan_by_id(&self, plan_id: &PlanId) -> Option<ArchivablePlan> {
        let plan_index = self.plan_id_index.lock().unwrap();
        if let Some(hash) = plan_index.get(plan_id) {
            self.retrieve_plan(hash).ok().flatten()
        } else {
            None
        }
    }

    /// Retrieve all plans for a given intent
    pub fn get_plans_for_intent(&self, intent_id: &IntentId) -> Vec<ArchivablePlan> {
        let intent_index = self.intent_id_index.lock().unwrap();
        if let Some(hashes) = intent_index.get(intent_id) {
            hashes
                .iter()
                .filter_map(|hash| self.retrieve_plan(hash).ok().flatten())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get archive statistics
    pub fn get_statistics(&self) -> PlanArchiveStatistics {
        let plan_count = {
            let plan_index = self.plan_id_index.lock().unwrap();
            plan_index.len()
        };

        let intent_count = {
            let intent_index = self.intent_id_index.lock().unwrap();
            intent_index.len()
        };

        let storage_size_bytes = match &self.storage {
            PlanArchiveStorage::InMemory(archive) => archive.size_bytes(),
            PlanArchiveStorage::File(archive) => {
                <IndexedArchive<ArchivablePlan, FileArchive> as ContentAddressableArchive<
                    ArchivablePlan,
                >>::stats(archive)
                .total_size_bytes
            }
        };

        PlanArchiveStatistics {
            total_plans: plan_count,
            total_intents_with_plans: intent_count,
            storage_size_bytes,
        }
    }

    /// List all archived plan IDs
    pub fn list_plan_ids(&self) -> Vec<PlanId> {
        let plan_index = self.plan_id_index.lock().unwrap();
        plan_index.keys().cloned().collect()
    }

    /// Check if a plan is archived
    pub fn contains_plan(&self, plan_id: &PlanId) -> bool {
        let plan_index = self.plan_id_index.lock().unwrap();
        plan_index.contains_key(plan_id)
    }
}

#[derive(Debug, Clone)]
pub struct PlanArchiveStatistics {
    pub total_plans: usize,
    pub total_intents_with_plans: usize,
    pub storage_size_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::PlanBody;
    use crate::PlanStatus;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_test_plan() -> Plan {
        Plan {
            plan_id: format!("plan_{}", uuid::Uuid::new_v4()),
            name: Some("Test Plan".to_string()),
            intent_ids: vec![format!("intent_{}", uuid::Uuid::new_v4())],
            language: crate::ccos::types::PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs("(println \"Hello World\")".to_string()),
            status: PlanStatus::Draft,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: Vec::new(),
            annotations: HashMap::new(),
        }
    }

    #[test]
    fn test_plan_archive_creation() {
        let archive = PlanArchive::new();
        let stats = archive.get_statistics();
        assert_eq!(stats.total_plans, 0);
        assert_eq!(stats.total_intents_with_plans, 0);
    }

    #[test]
    fn test_archive_and_retrieve_plan() {
        let archive = PlanArchive::new();
        let plan = create_test_plan();
        let plan_id = plan.plan_id.clone();

        // Archive the plan
        let hash = archive.archive_plan(&plan).unwrap();
        assert!(!hash.is_empty());

        // Retrieve the plan
        let retrieved = archive.get_plan_by_id(&plan_id).unwrap();
        assert_eq!(retrieved.plan_id, plan.plan_id);
        assert_eq!(retrieved.name, plan.name);
    }

    #[test]
    fn test_plans_for_intent() {
        let archive = PlanArchive::new();
        let intent_id = format!("intent_{}", uuid::Uuid::new_v4());

        // Create multiple plans for the same intent
        let mut plan1 = create_test_plan();
        plan1.intent_ids = vec![intent_id.clone()];
        let mut plan2 = create_test_plan();
        plan2.intent_ids = vec![intent_id.clone()];

        archive.archive_plan(&plan1).unwrap();
        archive.archive_plan(&plan2).unwrap();

        // Retrieve plans for intent
        let plans = archive.get_plans_for_intent(&intent_id);
        assert_eq!(plans.len(), 2);

        let plan_ids: Vec<String> = plans.iter().map(|p| p.plan_id.clone()).collect();
        assert!(plan_ids.contains(&plan1.plan_id));
        assert!(plan_ids.contains(&plan2.plan_id));
    }

    #[test]
    fn test_archive_statistics() {
        let archive = PlanArchive::new();
        let intent_id1 = format!("intent_{}", uuid::Uuid::new_v4());
        let intent_id2 = format!("intent_{}", uuid::Uuid::new_v4());

        let mut plan1 = create_test_plan();
        plan1.intent_ids = vec![intent_id1];
        let mut plan2 = create_test_plan();
        plan2.intent_ids = vec![intent_id2];

        archive.archive_plan(&plan1).unwrap();
        archive.archive_plan(&plan2).unwrap();

        let stats = archive.get_statistics();
        assert_eq!(stats.total_plans, 2);
        assert_eq!(stats.total_intents_with_plans, 2);
        assert!(stats.storage_size_bytes > 0);
    }

    #[test]
    fn test_contains_plan() {
        let archive = PlanArchive::new();
        let plan = create_test_plan();
        let plan_id = plan.plan_id.clone();

        assert!(!archive.contains_plan(&plan_id));

        archive.archive_plan(&plan).unwrap();
        assert!(archive.contains_plan(&plan_id));
    }

    #[test]
    fn test_list_plan_ids() {
        let archive = PlanArchive::new();
        let plan1 = create_test_plan();
        let plan2 = create_test_plan();

        archive.archive_plan(&plan1).unwrap();
        archive.archive_plan(&plan2).unwrap();

        let plan_ids = archive.list_plan_ids();
        assert_eq!(plan_ids.len(), 2);
        assert!(plan_ids.contains(&plan1.plan_id));
        assert!(plan_ids.contains(&plan2.plan_id));
    }

    #[test]
    fn test_persistence_and_rehydration() {
        let tmp = tempfile::tempdir().unwrap();
        let plan_dir = tmp.path().join("plans");
        std::fs::create_dir_all(&plan_dir).unwrap();
        // First lifecycle: create archive, add plan
        let archive1 =
            PlanArchive::with_file_storage(plan_dir.clone()).expect("create file archive");
        let plan = create_test_plan();
        let pid = plan.plan_id.clone();
        archive1.archive_plan(&plan).expect("archive plan");
        assert!(archive1.contains_plan(&pid));
        // Drop archive1 (goes out of scope) then recreate
        drop(archive1);
        let archive2 = PlanArchive::with_file_storage(plan_dir.clone()).expect("rehydrate archive");
        // Ensure indices were rehydrated
        assert!(
            archive2.contains_plan(&pid),
            "rehydrated archive should contain previously stored plan"
        );
        let retrieved = archive2.get_plan_by_id(&pid).expect("plan present");
        assert_eq!(retrieved.plan_id, pid);
    }

    #[test]
    fn test_rehydrate_recovers_from_corrupt_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let plan_dir = tmp.path().join("plans");
        std::fs::create_dir_all(&plan_dir).unwrap();

        // Create an archive and add a plan
        let archive1 =
            PlanArchive::with_file_storage(plan_dir.clone()).expect("create file archive");
        let plan = create_test_plan();
        let pid = plan.plan_id.clone();
        archive1.archive_plan(&plan).expect("archive plan");
        assert!(archive1.contains_plan(&pid));

        // Corrupt the plan_index.json on disk
        let index_path = plan_dir.join("plan_index.json");
        assert!(index_path.exists());
        std::fs::write(&index_path, b"{ this is not valid json: }").unwrap();

        // Reopen; rehydrate should detect parse error, scan stored plans, rebuild indices, and persist repaired sidecars
        drop(archive1);
        let archive2 = PlanArchive::with_file_storage(plan_dir.clone()).expect("rehydrate archive");
        assert!(
            archive2.contains_plan(&pid),
            "rehydrated archive should recover plan despite corrupt sidecar"
        );
        // Ensure repaired sidecar looks parseable
        let repaired = std::fs::read_to_string(plan_dir.join("plan_index.json")).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&repaired).expect("repaired sidecar JSON parse");
        assert!(parsed.get(&pid).is_some());
    }
}
