use serde::{Deserialize, Serialize};
use super::storage::Archivable;

/// Simplified Plan for archiving 
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivablePlan {
    pub plan_id: String,
    pub name: Option<String>,
}

impl Archivable for ArchivablePlan {
    fn entity_id(&self) -> String {
        self.plan_id.clone()
    }
    
    fn entity_type(&self) -> &'static str {
        "ArchivablePlan"
    }
}
