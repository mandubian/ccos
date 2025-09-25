use std::collections::{HashMap, HashSet};

/// Trust tiers categorize governance expectations for agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrustTier {
    T0Sandbox,
    T1Trusted,
    T2Privileged,
}

/// Basic latency stats summary.
#[derive(Debug, Clone, Default)]
pub struct LatencyStats {
    pub p50_ms: f64,
    pub p95_ms: f64,
}

/// Success & reliability metrics with decay-weighted historical performance.
#[derive(Debug, Clone, Default)]
pub struct SuccessStats {
    pub success_rate: f64,
    pub samples: u64,
    /// Decay-weighted success rate for adaptive threshold calculations
    pub decay_weighted_rate: f64,
    /// Decay factor for historical performance (0.0-1.0, higher = more recent bias)
    pub decay_factor: f64,
    /// Timestamp of last update for decay calculations
    pub last_update: Option<std::time::SystemTime>,
}

/// Cost model (simplified placeholder).
#[derive(Debug, Clone, Default)]
pub struct CostModel {
    pub cost_per_call: f64,
    pub tokens_per_second: f64,
}

/// Descriptor for a higher‑order cognitive agent able to accept intents.
#[derive(Debug, Clone)]
pub struct AgentDescriptor {
    pub agent_id: String,
    pub kind: String, // planner | analyzer | synthesizer | remote-arbiter | composite
    pub skills: Vec<String>,
    pub supported_constraints: Vec<String>,
    pub trust_tier: TrustTier,
    pub cost: CostModel,
    pub latency: LatencyStats,
    pub success: SuccessStats,
    pub provenance: Option<String>,
}

/// Scored candidate returned by registry queries.
#[derive(Debug, Clone)]
pub struct ScoredAgent {
    pub descriptor: AgentDescriptor,
    pub score: f64,
    pub rationale: String,
    pub skill_hits: u32,
}

/// Intent draft subset used for agent matching (placeholder until full Intent object integration).
#[derive(Debug, Clone)]
pub struct IntentDraft {
    pub goal: String,
    pub constraint_keys: Vec<String>,
}

/// Trait for agent lookup / registration.
pub trait AgentRegistry: Send + Sync {
    fn register(&mut self, agent: AgentDescriptor);
    fn list(&self) -> Vec<AgentDescriptor>;
    fn find_candidates(&self, draft: &IntentDraft, max: usize) -> Vec<ScoredAgent>;
    /// Record execution feedback for an agent (success/failure) updating rolling success stats.
    fn record_feedback(&mut self, agent_id: &str, success: bool);
}

/// In‑memory implementation with simple skill & constraint scoring.
pub struct InMemoryAgentRegistry {
    agents: HashMap<String, AgentDescriptor>,
    skill_index: HashMap<String, HashSet<String>>, // skill -> agent_ids
}

impl InMemoryAgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            skill_index: HashMap::new(),
        }
    }

    fn score_agent(agent: &AgentDescriptor, draft: &IntentDraft) -> (f64, String, u32) {
        // Skill overlap ratio
        let goal_lc = draft.goal.to_lowercase();
        let mut skill_hits = 0usize;
        for s in &agent.skills {
            if goal_lc.contains(&s.to_lowercase()) {
                skill_hits += 1;
            }
        }
        let skill_component = if agent.skills.is_empty() {
            0.0
        } else {
            skill_hits as f64 / agent.skills.len() as f64
        };

        // Constraint coverage
        let mut covered = 0usize;
        for c in &draft.constraint_keys {
            if agent.supported_constraints.iter().any(|ac| ac == c) {
                covered += 1;
            }
        }
        let constraint_component = if draft.constraint_keys.is_empty() {
            0.0
        } else {
            covered as f64 / draft.constraint_keys.len() as f64
        };

        // Trust weight
        let trust_weight = match agent.trust_tier {
            TrustTier::T0Sandbox => 0.6,
            TrustTier::T1Trusted => 0.85,
            TrustTier::T2Privileged => 1.0,
        };

        // Cost penalty (lower cost_per_call is better)
        let cost_penalty = (agent.cost.cost_per_call / 10.0).min(0.5); // heuristic scaling
        let base =
            (skill_component * 0.5 + constraint_component * 0.3 + agent.success.success_rate * 0.2)
                * trust_weight;
        let score = (base - cost_penalty).max(0.0);

        let rationale = format!(
            "skills={:.2} constraints={:.2} success={:.2} trust={} cost_penalty={:.2}",
            skill_component,
            constraint_component,
            agent.success.success_rate,
            match agent.trust_tier {
                TrustTier::T0Sandbox => "T0",
                TrustTier::T1Trusted => "T1",
                TrustTier::T2Privileged => "T2",
            },
            cost_penalty
        );
        (score, rationale, skill_hits as u32)
    }
}

impl AgentRegistry for InMemoryAgentRegistry {
    fn register(&mut self, agent: AgentDescriptor) {
        let id = agent.agent_id.clone();
        for s in &agent.skills {
            self.skill_index
                .entry(s.clone())
                .or_default()
                .insert(id.clone());
        }
        self.agents.insert(id, agent);
    }

    fn list(&self) -> Vec<AgentDescriptor> {
        self.agents.values().cloned().collect()
    }

    fn find_candidates(&self, draft: &IntentDraft, max: usize) -> Vec<ScoredAgent> {
        let mut scored: Vec<ScoredAgent> = self
            .agents
            .values()
            .map(|a| {
                let (score, rationale, skill_hits) = Self::score_agent(a, draft);
                ScoredAgent {
                    descriptor: a.clone(),
                    score,
                    rationale,
                    skill_hits,
                }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(max);
        scored
    }

    fn record_feedback(&mut self, agent_id: &str, success: bool) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            let stats = &mut agent.success;
            let successes_prior = stats.success_rate * stats.samples as f64;
            let successes_new = successes_prior + if success { 1.0 } else { 0.0 };
            stats.samples += 1;
            stats.success_rate = if stats.samples == 0 {
                0.0
            } else {
                successes_new / stats.samples as f64
            };
        }
    }
}
