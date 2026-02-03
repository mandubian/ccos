//! Skill Onboarding State Machine
//!
//! Manages the state of skill onboarding including:
//! - State transitions (NOT_LOADED → LOADED → NEEDS_SETUP → OPERATIONAL)
//! - Persistence of onboarding progress in WorkingMemory
//! - Integration with approval system for human-in-the-loop steps
//! - Automatic resumption after human actions

use crate::approval::types::ApprovalStatus;
use crate::approval::unified_queue::UnifiedApprovalQueue;
use serde::{Deserialize, Serialize};
use crate::skills::types::{
    HumanActionConfig, OnboardingState, OnboardingStep, OnboardingStepType, Skill, SkillOnboardingState,
};
use crate::working_memory::facade::WorkingMemory;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

/// Manages onboarding state for skills
pub struct OnboardingStateMachine {
    working_memory: Arc<StdMutex<WorkingMemory>>,
}

impl OnboardingStateMachine {
    /// Create a new state machine
    pub fn new(working_memory: Arc<StdMutex<WorkingMemory>>) -> Self {
        Self { working_memory }
    }

    /// Initialize onboarding for a skill
    pub fn initialize_onboarding(&self, skill: &Skill) -> Option<SkillOnboardingState> {
        let onboarding = skill.onboarding.as_ref()?;
        if !onboarding.required || onboarding.steps.is_empty() {
            return None;
        }

        let state = SkillOnboardingState::new(onboarding.steps.len());
        
        // Store initial state in working memory
        if let Err(e) = self.save_state(&skill.id, &state) {
            eprintln!("[Onboarding] Failed to save initial state: {}", e);
        }

        Some(state)
    }

    /// Get current onboarding state for a skill
    pub fn get_state(&self, skill_id: &str) -> Option<SkillOnboardingState> {
        let key = format!("skill:{}:onboarding_state", skill_id);
        
        let wm = self.working_memory.lock().ok()?;
        let entry = wm.get(&key).ok()??;
        
        serde_json::from_str(&entry.content).ok()
    }

    /// Save onboarding state to working memory
    fn save_state(&self, skill_id: &str, state: &SkillOnboardingState) -> Result<(), String> {
        let key = format!("skill:{}:onboarding_state", skill_id);
        let content = serde_json::to_string(state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        
        let entry_id = key.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = crate::working_memory::types::WorkingMemoryEntry::new_with_estimate(
            entry_id,
            format!("Onboarding state for {}", skill_id),
            content,
            vec!["onboarding".to_string(), format!("skill:{}", skill_id)],
            now,
            crate::working_memory::types::WorkingMemoryMeta::default(),
        );

        let mut wm = self.working_memory.lock()
            .map_err(|_| "Failed to lock working memory")?;
        wm.append(entry)
            .map_err(|e| format!("Failed to save state: {}", e))
    }

    /// Check if onboarding is required for a skill
    pub fn is_onboarding_required(&self, skill: &Skill) -> bool {
        match &skill.onboarding {
            Some(config) => config.required && !config.steps.is_empty(),
            None => false,
        }
    }

    /// Get the current step that needs to be executed
    pub fn get_current_step<'a>(
        &self,
        skill: &'a Skill,
        state: &SkillOnboardingState,
    ) -> Option<&'a OnboardingStep> {
        let onboarding = skill.onboarding.as_ref()?;
        onboarding.steps.get(state.current_step)
    }

    /// Check if a step's dependencies are satisfied
    pub fn are_dependencies_met(&self, step: &OnboardingStep, state: &SkillOnboardingState) -> bool {
        if step.depends_on.is_empty() {
            return true;
        }

        step.depends_on
            .iter()
            .all(|dep_id| state.completed_steps.contains(dep_id))
    }

    /// Execute an onboarding step (transition state)
    pub fn execute_step(
        &self,
        skill_id: &str,
        step: &OnboardingStep,
        state: &mut SkillOnboardingState,
    ) -> Result<StepExecutionResult, String> {
        // Check dependencies
        if !self.are_dependencies_met(step, state) {
            return Err(format!(
                "Dependencies not met for step '{}'. Required: {:?}, Completed: {:?}",
                step.id, step.depends_on, state.completed_steps
            ));
        }

        let result = match &step.step_type {
            OnboardingStepType::ApiCall => {
                // API call steps are executed by the agent
                // Return info about what operation to call
                StepExecutionResult::ApiCall {
                    operation: step.operation.clone().unwrap_or_default(),
                    params: step.params.clone(),
                }
            }
            OnboardingStepType::HumanAction => {
                // Human action steps require approval
                if let Some(action_config) = &step.action {
                    state.set_pending_human_action(String::new()); // Approval ID set by caller
                    StepExecutionResult::HumanAction {
                        action_config: action_config.clone(),
                    }
                } else {
                    return Err(format!("Human action step '{}' missing action config", step.id));
                }
            }
            OnboardingStepType::Condition => {
                // Condition steps check requirements
                StepExecutionResult::CheckCondition
            }
        };

        // Save updated state
        self.save_state(skill_id, state)?;

        Ok(result)
    }

    /// Complete a step and advance to next
    pub fn complete_step(
        &self,
        skill_id: &str,
        step_id: String,
        state: &mut SkillOnboardingState,
        data: HashMap<String, serde_json::Value>,
    ) -> Result<(), String> {
        // Store collected data
        for (key, value) in data {
            state.data.insert(key, value);
        }

        // Mark step complete
        state.complete_step(step_id);

        // Save state
        self.save_state(skill_id, state)?;

        Ok(())
    }

    /// Check and resume onboarding after human action approval
    pub async fn check_and_resume<S: crate::approval::types::ApprovalStorage>(
        &self,
        skill_id: &str,
        approval_queue: &UnifiedApprovalQueue<S>,
    ) -> Result<Option<SkillOnboardingState>, String> {
        let mut state = self.get_state(skill_id)
            .ok_or_else(|| format!("No onboarding state found for skill {}", skill_id))?;

        if state.status != OnboardingState::PendingHumanAction {
            return Ok(Some(state));
        }

        let approval_id = state.pending_approval_id.clone()
            .ok_or_else(|| "No pending approval ID")?;

        // Check approval status
        let approval = approval_queue
            .get(&approval_id)
            .await
            .map_err(|e| format!("Failed to get approval: {}", e))?
            .ok_or_else(|| format!("Approval {} not found", approval_id))?;

        match approval.status {
            ApprovalStatus::Approved { .. } => {
                // Human action completed - resume onboarding
                state.resume_from_human_action();
                
                // Store the response data if available
                if let Some(response) = &approval.response {
                    state.data.insert(format!("approval_{}_response", approval_id), response.clone());
                }

                self.save_state(skill_id, &state)?;
                Ok(Some(state))
            }
            ApprovalStatus::Rejected { .. } => {
                Err(format!("Human action approval {} was rejected", approval_id))
            }
            ApprovalStatus::Expired { .. } => {
                Err(format!("Human action approval {} expired", approval_id))
            }
            _ => {
                // Still pending
                Ok(Some(state))
            }
        }
    }

    /// Get onboarding status summary
    pub fn get_status_summary(&self, skill_id: &str) -> Option<OnboardingStatusSummary> {
        let state = self.get_state(skill_id)?;
        
        Some(OnboardingStatusSummary {
            status: state.status.clone(),
            current_step: state.current_step,
            total_steps: state.total_steps,
            completed_steps: state.completed_steps.clone(),
            is_complete: state.status == OnboardingState::Operational,
            pending_approval: state.pending_approval_id.clone(),
        })
    }

    /// Reset onboarding for a skill (start over)
    pub fn reset_onboarding(&self, skill: &Skill) -> Option<SkillOnboardingState> {
        let state = self.initialize_onboarding(skill)?;
        
        // Clear any existing state first
        let key = format!("skill:{}:onboarding_state", skill.id);
        if let Ok(wm) = self.working_memory.lock() {
            // Try to remove old entry (ignore errors)
            let _ = wm.get(&key);
        }

        self.save_state(&skill.id, &state).ok()?;
        Some(state)
    }
}

/// Result of executing an onboarding step
#[derive(Debug, Clone)]
pub enum StepExecutionResult {
    /// Execute an API call operation
    ApiCall {
        operation: String,
        params: HashMap<String, String>,
    },
    /// Request human action
    HumanAction {
        action_config: HumanActionConfig,
    },
    /// Check a condition
    CheckCondition,
}

/// Summary of onboarding status for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingStatusSummary {
    pub status: OnboardingState,
    pub current_step: usize,
    pub total_steps: usize,
    pub completed_steps: Vec<String>,
    pub is_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_approval: Option<String>,
}
