use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::storage::IntentGraphStorage;
use super::virtualization::VirtualizationConfig;
use crate::intent_storage::IntentFilter;
use crate::types::{EdgeType, ExecutionResult, IntentId, IntentStatus, StorableIntent};
use rtfs::runtime::RuntimeError;

/// Intent summarization for virtualization
#[derive(Debug)]
pub struct IntentSummarizer {
    max_summary_length: usize,
}

impl Default for IntentSummarizer {
    fn default() -> Self {
        Self {
            max_summary_length: 500,
        }
    }
}

impl IntentSummarizer {
    pub fn new(max_summary_length: usize) -> Self {
        Self { max_summary_length }
    }

    /// Summarize a cluster of intents
    pub fn summarize_cluster(&self, intents: &[StorableIntent]) -> String {
        if intents.is_empty() {
            return "Empty cluster".to_string();
        }

        let key_goals = self.extract_key_goals(intents);
        let dominant_status = self.determine_dominant_status(intents);
        let description = self.generate_cluster_description(intents, &key_goals, &dominant_status);

        // Truncate if necessary
        if description.len() > self.max_summary_length {
            format!("{}...", &description[..self.max_summary_length])
        } else {
            description
        }
    }

    /// Create cluster summaries for multiple clusters
    pub async fn create_cluster_summary(
        &self,
        cluster: &[IntentId],
        storage: &IntentGraphStorage,
    ) -> Result<String, RuntimeError> {
        if cluster.is_empty() {
            return Ok("Empty cluster".to_string());
        }

        let mut summary_parts = Vec::new();

        for intent_id in cluster.iter().take(5) {
            // Limit to 5 intents per cluster
            if let Ok(Some(intent)) = storage.get_intent(intent_id).await {
                let intent_summary = format!(
                    "- {}: {}",
                    intent.name.unwrap_or_else(|| "Unnamed".to_string()),
                    intent.goal.chars().take(100).collect::<String>()
                );
                summary_parts.push(intent_summary);
            }
        }

        if cluster.len() > 5 {
            summary_parts.push(format!("... and {} more intents", cluster.len() - 5));
        }

        let full_summary = summary_parts.join("\n");

        // Truncate to max length
        if full_summary.len() > self.max_summary_length {
            Ok(format!("{}...", &full_summary[..self.max_summary_length]))
        } else {
            Ok(full_summary)
        }
    }

    /// Calculate cluster relevance score
    pub fn calculate_cluster_relevance(&self, intents: &[StorableIntent]) -> f64 {
        if intents.is_empty() {
            return 0.0;
        }

        let status_weights = intents
            .iter()
            .map(|intent| match intent.status {
                IntentStatus::Active => 1.0,
                IntentStatus::Executing => 0.95,
                IntentStatus::Failed => 0.8,
                IntentStatus::Suspended => 0.6,
                IntentStatus::Completed => 0.4,
                IntentStatus::Archived => 0.2,
            })
            .collect::<Vec<_>>();

        // Calculate average status relevance
        let avg_status_relevance = status_weights.iter().sum::<f64>() / status_weights.len() as f64;

        // Factor in cluster size (larger clusters might be more important)
        let size_factor = (intents.len() as f64).log10().max(1.0);

        // Factor in recency
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let avg_age = intents
            .iter()
            .map(|intent| now.saturating_sub(intent.created_at))
            .sum::<u64>() as f64
            / intents.len() as f64;

        let recency_factor = 1.0 / (1.0 + avg_age / 86400.0 * 0.1); // Age in days

        avg_status_relevance * size_factor * recency_factor
    }

    /// Extract key goals from a cluster
    fn extract_key_goals(&self, intents: &[StorableIntent]) -> Vec<String> {
        // Group similar goals (simple keyword extraction)
        let mut goal_keywords = HashMap::new();

        for intent in intents {
            let words: Vec<&str> = intent
                .goal
                .split_whitespace()
                .filter(|word| word.len() > 3) // Filter short words
                .take(5) // Take up to 5 words per goal
                .collect();

            for word in words {
                let normalized = word.to_lowercase();
                *goal_keywords.entry(normalized).or_insert(0) += 1;
            }
        }

        // Get the most common keywords
        let mut sorted_keywords: Vec<_> = goal_keywords.into_iter().collect();
        sorted_keywords.sort_by(|a, b| b.1.cmp(&a.1));

        sorted_keywords
            .into_iter()
            .take(3) // Top 3 keywords
            .map(|(keyword, _)| keyword)
            .collect()
    }

    /// Determine the dominant status in a cluster
    fn determine_dominant_status(&self, intents: &[StorableIntent]) -> IntentStatus {
        let mut status_counts = HashMap::new();

        for intent in intents {
            *status_counts.entry(intent.status.clone()).or_insert(0) += 1;
        }

        status_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(status, _)| status)
            .unwrap_or(IntentStatus::Active) // Default fallback
    }

    /// Generate a description for the cluster
    fn generate_cluster_description(
        &self,
        intents: &[StorableIntent],
        key_goals: &[String],
        dominant_status: &IntentStatus,
    ) -> String {
        let count = intents.len();
        let goals_text = if key_goals.is_empty() {
            "various goals".to_string()
        } else {
            key_goals.join(", ")
        };

        let status_text = match dominant_status {
            IntentStatus::Active => "actively being pursued",
            IntentStatus::Executing => "currently executing",
            IntentStatus::Completed => "completed",
            IntentStatus::Failed => "failed",
            IntentStatus::Suspended => "suspended",
            IntentStatus::Archived => "archived",
        };

        format!(
            "Cluster of {} intents related to {} ({})",
            count, goals_text, status_text
        )
    }
}

/// Intent pruning engine for managing large graphs
#[allow(dead_code)]
#[derive(Debug)]
pub struct IntentPruningEngine {
    importance_threshold: f64,
    age_threshold_days: u64,
}

impl Default for IntentPruningEngine {
    fn default() -> Self {
        Self {
            importance_threshold: 0.3,
            age_threshold_days: 365,
        }
    }
}

impl IntentPruningEngine {
    pub fn new(importance_threshold: f64, age_threshold_days: u64) -> Self {
        Self {
            importance_threshold,
            age_threshold_days,
        }
    }

    /// Prune intents based on criteria
    pub fn prune_intents(
        &self,
        relevant_intents: &[IntentId],
        _storage: &IntentGraphStorage,
        _config: &VirtualizationConfig,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        // For now, just return the input intents (no actual pruning)
        // In a full implementation, this would apply sophisticated pruning logic
        Ok(relevant_intents.to_vec())
    }

    /// Identify intents that can be pruned
    pub fn identify_prunable_intents(
        &self,
        intent_ids: &[IntentId],
        storage: &IntentGraphStorage,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let mut prunable = Vec::new();

        for intent_id in intent_ids {
            if let Some(intent) = storage.get_intent_sync(intent_id) {
                let importance = self.calculate_importance(&intent);
                if importance < self.importance_threshold {
                    prunable.push(intent_id.clone());
                }
            }
        }

        Ok(prunable)
    }

    /// Calculate importance score for an intent
    fn calculate_importance(&self, intent: &StorableIntent) -> f64 {
        let mut score = 0.0;

        // Status-based scoring
        score += match intent.status {
            IntentStatus::Active => 1.0,
            IntentStatus::Executing => 0.95,
            IntentStatus::Suspended => 0.8,
            IntentStatus::Failed => 0.6,
            IntentStatus::Completed => 0.4,
            IntentStatus::Archived => 0.1,
        };

        // Priority from metadata
        if let Some(priority_str) = intent.metadata.get("priority") {
            if let Ok(priority) = priority_str.parse::<f64>() {
                score += priority * 0.3;
            }
        }

        // Recency factor
        let age_days = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - intent.created_at)
            / 86400;
        let recency_score = 1.0 / (1.0 + age_days as f64 * 0.05);
        score += recency_score * 0.5;

        // Goal complexity score (more complex goals might be more important)
        let complexity_score = (intent.goal.len() as f64 / 100.0).min(1.0);
        score += complexity_score * 0.2;

        score
    }

    /// Remove intents below relevance threshold
    pub fn filter_by_relevance(
        &self,
        intent_ids: &[IntentId],
        storage: &IntentGraphStorage,
        threshold: f64,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let mut filtered = Vec::new();

        for intent_id in intent_ids {
            if let Some(intent) = storage.get_intent_sync(intent_id) {
                let score = self.calculate_base_relevance(&intent);
                if score >= threshold {
                    filtered.push(intent_id.clone());
                }
            }
        }

        Ok(filtered)
    }

    /// Calculate base relevance score for an intent
    fn calculate_base_relevance(&self, intent: &StorableIntent) -> f64 {
        match intent.status {
            IntentStatus::Active => 0.9,
            IntentStatus::Executing => 0.85,
            IntentStatus::Failed => 0.7,
            IntentStatus::Suspended => 0.5,
            IntentStatus::Completed => 0.3,
            IntentStatus::Archived => 0.1,
        }
    }
}

/// Lifecycle management for intents
#[derive(Debug)]
pub struct IntentLifecycleManager;

impl IntentLifecycleManager {
    /// Archive completed intents (existing functionality)
    pub async fn archive_completed_intents(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
    ) -> Result<(), RuntimeError> {
        let completed_filter = IntentFilter {
            status: Some(IntentStatus::Completed),
            ..Default::default()
        };

        let completed_intents = storage.list_intents(completed_filter).await?;

        for mut intent in completed_intents {
            self.transition_intent_status(
                storage,
                event_sink,
                &mut intent,
                IntentStatus::Archived,
                "Auto-archived completed intent".to_string(),
                None, // triggering_plan_id - will be enhanced later
            )
            .await?;
        }

        Ok(())
    }

    /// Transition an intent to a new status with audit trail
    pub async fn transition_intent_status(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent: &mut StorableIntent,
        new_status: IntentStatus,
        reason: String,
        triggering_plan_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let old_status = intent.status.clone();

        // Validate the transition
        self.validate_status_transition(&old_status, &new_status)?;

        // Update the intent
        intent.status = new_status.clone();
        intent.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Add audit trail to metadata
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Count existing transitions to ensure unique keys
        let transition_count = intent
            .metadata
            .keys()
            .filter(|key| key.starts_with("status_transition_"))
            .count();

        let audit_entry = format!(
            "{}: {} -> {} (reason: {})",
            timestamp,
            self.status_to_string(&old_status),
            self.status_to_string(&new_status),
            reason
        );

        let audit_key = format!("status_transition_{}_{}", timestamp, transition_count);
        intent.metadata.insert(audit_key, audit_entry);

        // Store the updated intent
        storage.update_intent(intent).await?;

        // Emit status change event for mandatory audit
        // NOTE: We don't yet have a concrete plan_id wired here; pass empty string until orchestration plumbs it.
        event_sink.log_intent_status_change(
            "", // plan_id placeholder
            &intent.intent_id,
            self.status_to_string(&old_status),
            self.status_to_string(&new_status),
            &reason,
            triggering_plan_id,
        )?;

        Ok(())
    }

    /// Complete an intent (transition to Completed status)
    pub async fn complete_intent(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_id: &IntentId,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        let (target_status, reason) = if result.success {
            (
                IntentStatus::Completed,
                "Intent completed successfully".to_string(),
            )
        } else {
            (
                IntentStatus::Failed,
                format!("Intent failed with errors: {:?}", result.value),
            )
        };

        self.transition_intent_status(
            storage,
            event_sink,
            &mut intent,
            target_status,
            reason,
            None, // triggering_plan_id - will be enhanced later
        )
        .await?;

        Ok(())
    }

    /// Fail an intent (transition to Failed status)
    pub async fn fail_intent(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_id: &IntentId,
        error_message: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        self.transition_intent_status(
            storage,
            event_sink,
            &mut intent,
            IntentStatus::Failed,
            format!("Intent failed: {}", error_message),
            None, // triggering_plan_id - will be enhanced later
        )
        .await?;

        Ok(())
    }

    /// Suspend an intent (transition to Suspended status)
    pub async fn suspend_intent(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        self.transition_intent_status(
            storage,
            event_sink,
            &mut intent,
            IntentStatus::Suspended,
            format!("Intent suspended: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        )
        .await?;

        Ok(())
    }

    /// Resume a suspended intent (transition to Active status)
    pub async fn resume_intent(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        self.transition_intent_status(
            storage,
            event_sink,
            &mut intent,
            IntentStatus::Active,
            format!("Intent resumed: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        )
        .await?;

        Ok(())
    }

    /// Archive an intent (transition to Archived status)
    pub async fn archive_intent(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        self.transition_intent_status(
            storage,
            event_sink,
            &mut intent,
            IntentStatus::Archived,
            format!("Intent archived: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        )
        .await?;

        Ok(())
    }

    /// Reactivate an archived intent (transition to Active status)
    pub async fn reactivate_intent(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        self.transition_intent_status(
            storage,
            event_sink,
            &mut intent,
            IntentStatus::Active,
            format!("Intent reactivated: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        )
        .await?;

        Ok(())
    }

    /// Get intents by status
    pub async fn get_intents_by_status(
        &self,
        storage: &IntentGraphStorage,
        status: IntentStatus,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let filter = IntentFilter {
            status: Some(status),
            ..Default::default()
        };

        storage.list_intents(filter).await
    }

    /// Get intent status transition history
    pub async fn get_status_history(
        &self,
        storage: &IntentGraphStorage,
        intent_id: &IntentId,
    ) -> Result<Vec<String>, RuntimeError> {
        let intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        let mut history = Vec::new();

        // Extract status transition entries from metadata
        for (key, value) in &intent.metadata {
            if key.starts_with("status_transition_") {
                history.push(value.clone());
            }
        }

        // Sort by timestamp (extracted from key)
        history.sort_by(|a, b| {
            let timestamp_a = a
                .split(':')
                .next()
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or(0);
            let timestamp_b = b
                .split(':')
                .next()
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or(0);
            timestamp_a.cmp(&timestamp_b)
        });

        Ok(history)
    }

    /// Validate if a status transition is allowed
    fn validate_status_transition(
        &self,
        from: &IntentStatus,
        to: &IntentStatus,
    ) -> Result<(), RuntimeError> {
        match (from, to) {
            // Active can transition to Executing, Suspended, Archived, Failed (edge), Completed (unlikely direct), or stay Active
            (IntentStatus::Active, _) => Ok(()),

            // Executing can complete, fail, suspend, return to Active (rollback), or archive (edge case)
            (IntentStatus::Executing, IntentStatus::Completed) => Ok(()),
            (IntentStatus::Executing, IntentStatus::Failed) => Ok(()),
            (IntentStatus::Executing, IntentStatus::Suspended) => Ok(()),
            (IntentStatus::Executing, IntentStatus::Active) => Ok(()),
            (IntentStatus::Executing, IntentStatus::Archived) => Ok(()),
            (IntentStatus::Executing, _) => Err(RuntimeError::Generic(format!(
                "Cannot transition from Executing to {:?}",
                to
            ))),

            // Completed can only transition to Archived
            (IntentStatus::Completed, IntentStatus::Archived) => Ok(()),
            (IntentStatus::Completed, _) => Err(RuntimeError::Generic(format!(
                "Cannot transition from Completed to {:?}",
                to
            ))),

            // Failed can transition to Active (retry) or Archived
            (IntentStatus::Failed, IntentStatus::Active) => Ok(()),
            (IntentStatus::Failed, IntentStatus::Archived) => Ok(()),
            (IntentStatus::Failed, _) => Err(RuntimeError::Generic(format!(
                "Cannot transition from Failed to {:?}",
                to
            ))),

            // Suspended can transition to Active (resume) or Archived
            (IntentStatus::Suspended, IntentStatus::Active) => Ok(()),
            (IntentStatus::Suspended, IntentStatus::Archived) => Ok(()),
            (IntentStatus::Suspended, _) => Err(RuntimeError::Generic(format!(
                "Cannot transition from Suspended to {:?}",
                to
            ))),

            // Archived can transition to Active (reactivate)
            (IntentStatus::Archived, IntentStatus::Active) => Ok(()),
            (IntentStatus::Archived, _) => Err(RuntimeError::Generic(format!(
                "Cannot transition from Archived to {:?}",
                to
            ))),
        }
    }

    /// Convert status to string for audit trail
    fn status_to_string(&self, status: &IntentStatus) -> &'static str {
        match status {
            IntentStatus::Active => "Active",
            IntentStatus::Executing => "Executing",
            IntentStatus::Completed => "Completed",
            IntentStatus::Failed => "Failed",
            IntentStatus::Archived => "Archived",
            IntentStatus::Suspended => "Suspended",
        }
    }

    /// Get intents that are ready for processing (Active status)
    pub async fn get_ready_intents(
        &self,
        storage: &IntentGraphStorage,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        self.get_intents_by_status(storage, IntentStatus::Active)
            .await
    }

    /// Get intents that need attention (Failed or Suspended status)
    pub async fn get_intents_needing_attention(
        &self,
        storage: &IntentGraphStorage,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let failed = self
            .get_intents_by_status(storage, IntentStatus::Failed)
            .await?;
        let suspended = self
            .get_intents_by_status(storage, IntentStatus::Suspended)
            .await?;

        let mut needing_attention = failed;
        needing_attention.extend(suspended);

        Ok(needing_attention)
    }

    /// Get intents that can be archived (Completed for more than specified days)
    pub async fn get_intents_ready_for_archival(
        &self,
        storage: &IntentGraphStorage,
        days_threshold: u64,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let completed_intents = self
            .get_intents_by_status(storage, IntentStatus::Completed)
            .await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let threshold_seconds = days_threshold * 24 * 60 * 60;

        let ready_for_archival = completed_intents
            .into_iter()
            .filter(|intent| {
                let time_since_completion = now.saturating_sub(intent.updated_at);
                time_since_completion >= threshold_seconds
            })
            .collect();

        Ok(ready_for_archival)
    }

    /// Bulk transition intents by status
    pub async fn bulk_transition_intents(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_ids: &[IntentId],
        new_status: IntentStatus,
        reason: String,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let mut successful_transitions = Vec::new();
        let mut errors = Vec::new();

        for intent_id in intent_ids {
            match self
                .transition_intent_by_id(
                    storage,
                    event_sink,
                    intent_id,
                    new_status.clone(),
                    reason.clone(),
                )
                .await
            {
                Ok(()) => successful_transitions.push(intent_id.clone()),
                Err(e) => errors.push((intent_id.clone(), e)),
            }
        }

        if !errors.is_empty() {
            let error_summary = errors
                .iter()
                .map(|(id, e)| format!("{}: {}", id, e))
                .collect::<Vec<_>>()
                .join(", ");

            return Err(RuntimeError::Generic(format!(
                "Some transitions failed: {}",
                error_summary
            )));
        }

        Ok(successful_transitions)
    }

    /// Helper method to transition intent by ID
    async fn transition_intent_by_id(
        &self,
        storage: &mut IntentGraphStorage,
        event_sink: &dyn crate::event_sink::IntentEventSink,
        intent_id: &IntentId,
        new_status: IntentStatus,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage
            .get_intent(intent_id)
            .await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;

        self.transition_intent_status(
            storage,
            event_sink,
            &mut intent,
            new_status,
            reason,
            None, // triggering_plan_id - will be enhanced later
        )
        .await
    }

    /// Infer edges between intents (existing functionality)
    pub async fn infer_edges(&self, storage: &mut IntentGraphStorage) -> Result<(), RuntimeError> {
        // Simple edge inference based on goal similarity
        // In a full implementation, this would use more sophisticated NLP

        let all_intents = storage.list_intents(IntentFilter::default()).await?;

        for i in 0..all_intents.len() {
            for j in (i + 1)..all_intents.len() {
                let intent_a = &all_intents[i];
                let intent_b = &all_intents[j];

                // Check for potential conflicts based on resource constraints
                if self.detect_resource_conflict(intent_a, intent_b) {
                    let edge = super::storage::Edge::new(
                        intent_a.intent_id.clone(),
                        intent_b.intent_id.clone(),
                        EdgeType::ConflictsWith,
                    );
                    storage.store_edge(edge).await?;
                }
            }
        }

        Ok(())
    }

    fn detect_resource_conflict(
        &self,
        intent_a: &StorableIntent,
        intent_b: &StorableIntent,
    ) -> bool {
        // Simple conflict detection based on cost constraints
        let cost_a = intent_a
            .constraints
            .get("max_cost")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::INFINITY);
        let cost_b = intent_b
            .constraints
            .get("max_cost")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::INFINITY);

        // If both have very low cost constraints, they might conflict
        cost_a < 10.0 && cost_b < 10.0
    }
}
