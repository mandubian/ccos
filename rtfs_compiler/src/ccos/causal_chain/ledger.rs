use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use super::super::types::{Action, ActionId, ActionType, CapabilityId, IntentId, PlanId};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Immutable ledger storage
#[derive(Debug)]
pub struct ImmutableLedger {
    // In a full implementation, this would be append-only file storage
    pub actions: Vec<Action>,
    pub hash_chain: Vec<String>,
    pub indices: LedgerIndices,
}

impl ImmutableLedger {
    /// Get children actions for a given parent_action_id
    pub fn get_children(&self, parent_id: &ActionId) -> Vec<&Action> {
        self.indices.get_children(parent_id)
            .iter()
            .filter_map(|id| self.get_action(id))
            .collect()
    }

    /// Get parent action for a given action_id
    pub fn get_parent(&self, action_id: &ActionId) -> Option<&Action> {
        self.get_action(action_id)
            .and_then(|action| action.parent_action_id.as_ref())
            .and_then(|parent_id| self.get_action(parent_id))
    }
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            hash_chain: Vec::new(),
            indices: LedgerIndices::new(),
        }
    }

    pub fn append_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        // Calculate hash for this action
        let action_hash = self.calculate_action_hash(action);

        // Calculate the chain hash (includes previous hash)
        let chain_hash = self.calculate_chain_hash(&action_hash);

        // Append to ledger
        self.actions.push(action.clone());
        self.hash_chain.push(chain_hash);

        // Update indices
        self.indices.index_action(action)?;

        Ok(())
    }

    pub fn append(&mut self, action: &Action) -> Result<String, RuntimeError> {
        self.append_action(action)?;
        Ok(action.action_id.clone())
    }

    pub fn get_action(&self, action_id: &ActionId) -> Option<&Action> {
    // If an action is appended multiple times with the same ID (e.g.,
    // initial log then a later result record), prefer the most recent one.
    self.actions.iter().rev().find(|a| a.action_id == *action_id)
    }

    pub fn get_actions_by_intent(&self, intent_id: &IntentId) -> Vec<&Action> {
        self.indices
            .intent_actions
            .get(intent_id)
            .map(|action_ids| {
                action_ids
                    .iter()
                    .filter_map(|id| self.get_action(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_actions_by_plan(&self, plan_id: &PlanId) -> Vec<&Action> {
        self.indices
            .plan_actions
            .get(plan_id)
            .map(|action_ids| {
                action_ids
                    .iter()
                    .filter_map(|id| self.get_action(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_actions_by_capability(&self, capability_id: &CapabilityId) -> Vec<&Action> {
        self.indices
            .capability_actions
            .get(capability_id)
            .map(|action_ids| {
                action_ids
                    .iter()
                    .filter_map(|id| self.get_action(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_all_actions(&self) -> &[Action] {
        &self.actions
    }

    pub fn verify_integrity(&self) -> Result<bool, RuntimeError> {
        let mut last_chain_hash: Option<&String> = None;
        for (i, action) in self.actions.iter().enumerate() {
            let action_hash = self.calculate_action_hash(action);
            let mut hasher = Sha256::new();
            if let Some(prev_hash) = last_chain_hash {
                hasher.update(prev_hash.as_bytes());
            }
            hasher.update(action_hash.as_bytes());
            let expected_chain_hash = format!("{:x}", hasher.finalize());

            if self.hash_chain[i] != expected_chain_hash {
                return Ok(false);
            }
            last_chain_hash = Some(&self.hash_chain[i]);
        }
        Ok(true)
    }

    fn calculate_action_hash(&self, action: &Action) -> String {
        let mut hasher = Sha256::new();

        // Hash all action fields
        hasher.update(action.action_id.as_bytes());
        hasher.update(action.plan_id.as_bytes());
        hasher.update(action.intent_id.as_bytes());
        if let Some(function_name) = &action.function_name {
            hasher.update(function_name.as_bytes());
        }
        hasher.update(action.timestamp.to_string().as_bytes());

        // Hash arguments
        if let Some(args) = &action.arguments {
            for arg in args { hasher.update(format!("{:?}", arg).as_bytes()); }
        }

        // Hash result
        if let Some(result) = &action.result {
            hasher.update(format!("{:?}", result).as_bytes());
        }

        // Hash metadata
        for (key, value) in &action.metadata {
            hasher.update(key.as_bytes());
            hasher.update(format!("{:?}", value).as_bytes());
        }

        format!("{:x}", hasher.finalize())
    }

    fn calculate_chain_hash(&self, action_hash: &str) -> String {
        let mut hasher = Sha256::new();

        // Include previous hash in chain
        if let Some(prev_hash) = self.hash_chain.last() {
            hasher.update(prev_hash.as_bytes());
        }

        hasher.update(action_hash.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// Indices for fast lookup
#[derive(Debug)]
pub struct LedgerIndices {
    pub intent_actions: HashMap<IntentId, Vec<ActionId>>,
    pub plan_actions: HashMap<PlanId, Vec<ActionId>>,
    pub capability_actions: HashMap<CapabilityId, Vec<ActionId>>,
    pub function_actions: HashMap<String, Vec<ActionId>>,
    pub timestamp_index: Vec<ActionId>, // Chronological order
    pub parent_to_children: HashMap<ActionId, Vec<ActionId>>, // Tree traversal
}

impl LedgerIndices {
    pub fn get_children(&self, parent_id: &ActionId) -> Vec<ActionId> {
        self.parent_to_children
            .get(parent_id)
            .cloned()
            .unwrap_or_default()
    }
    pub fn new() -> Self {
        Self {
            intent_actions: HashMap::new(),
            plan_actions: HashMap::new(),
            capability_actions: HashMap::new(),
            function_actions: HashMap::new(),
            timestamp_index: Vec::new(),
            parent_to_children: HashMap::new(),
        }
    }

    pub fn index_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        // Index by intent
        self.intent_actions
            .entry(action.intent_id.clone())
            .or_insert_with(Vec::new)
            .push(action.action_id.clone());

        // Index by plan
        self.plan_actions
            .entry(action.plan_id.clone())
            .or_insert_with(Vec::new)
            .push(action.action_id.clone());

    // Index by capability (stored in function_name for capability calls/results)
    if action.action_type == ActionType::CapabilityCall || action.action_type == ActionType::CapabilityResult {
            if let Some(function_name) = &action.function_name {
                self.capability_actions
                    .entry(function_name.clone())
                    .or_insert_with(Vec::new)
                    .push(action.action_id.clone());
            }
        }

        // Index by function
        if let Some(function_name) = &action.function_name {
            self.function_actions
                .entry(function_name.clone())
                .or_insert_with(Vec::new)
                .push(action.action_id.clone());
        }

        // Index by timestamp
        self.timestamp_index.push(action.action_id.clone());

        // Index by parent_action_id for tree traversal
        if let Some(parent_id) = &action.parent_action_id {
            self.parent_to_children
                .entry(parent_id.clone())
                .or_insert_with(Vec::new)
                .push(action.action_id.clone());
        }

        Ok(())
    }
}
