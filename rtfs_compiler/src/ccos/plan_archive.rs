use super::storage::{ContentAddressableArchive, InMemoryArchive};
use super::archivable_types::ArchivablePlan;
use super::types::{Plan, PlanId, IntentId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Domain-specific Plan archive with indexing and retrieval capabilities
pub struct PlanArchive {
    storage: InMemoryArchive<ArchivablePlan>,
    plan_id_index: Arc<Mutex<HashMap<PlanId, String>>>,
    intent_id_index: Arc<Mutex<HashMap<IntentId, Vec<String>>>>,
}

impl PlanArchive {
    pub fn new() -> Self {
        Self {
            storage: InMemoryArchive::new(),
            plan_id_index: Arc::new(Mutex::new(HashMap::new())),
            intent_id_index: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Archive a plan and update indices
    pub fn archive_plan(&self, plan: &Plan) -> Result<String, String> {
        let archivable_plan = ArchivablePlan::from(plan);
        let hash = self.storage.store(archivable_plan.clone())?;

        // Update plan_id index
        {
            let mut plan_index = self.plan_id_index.lock().unwrap();
            plan_index.insert(plan.plan_id.clone(), hash.clone());
        }

        // Update intent_id index for all intent IDs associated with this plan
        {
            let mut intent_index = self.intent_id_index.lock().unwrap();
            for intent_id in &plan.intent_ids {
                intent_index.entry(intent_id.clone())
                    .or_insert_with(Vec::new)
                    .push(hash.clone());
            }
        }

        Ok(hash)
    }

    /// Retrieve a plan by its ID
    pub fn get_plan_by_id(&self, plan_id: &PlanId) -> Option<ArchivablePlan> {
        let plan_index = self.plan_id_index.lock().unwrap();
        if let Some(hash) = plan_index.get(plan_id) {
            self.storage.retrieve(hash).ok().flatten()
        } else {
            None
        }
    }

    /// Retrieve all plans for a given intent
    pub fn get_plans_for_intent(&self, intent_id: &IntentId) -> Vec<ArchivablePlan> {
        let intent_index = self.intent_id_index.lock().unwrap();
        if let Some(hashes) = intent_index.get(intent_id) {
            hashes.iter()
                .filter_map(|hash| self.storage.retrieve(hash).ok().flatten())
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

        PlanArchiveStatistics {
            total_plans: plan_count,
            total_intents_with_plans: intent_count,
            storage_size_bytes: self.storage.size_bytes(),
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
    use crate::runtime::values::Value;
    use std::time::{SystemTime, UNIX_EPOCH};
    use crate::PlanBody;
    use crate::PlanStatus;

    fn create_test_plan() -> Plan {
        Plan {
            plan_id: format!("plan_{}", uuid::Uuid::new_v4()),
            name: Some("Test Plan".to_string()),
            intent_ids: vec![format!("intent_{}", uuid::Uuid::new_v4())],
            language: crate::ccos::types::PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs("(println \"Hello World\")".to_string()),
            status: PlanStatus::Draft,
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
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
}
