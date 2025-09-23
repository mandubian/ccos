# Delegation Configuration Guide

## Overview

This document describes the delegation configuration system in CCOS, which allows agents to delegate tasks to specialized agents based on capability matching and trust scores.

## Architecture

The delegation system consists of several components:

1. **AgentConfig DelegationConfig** - Top-level configuration in agent config
2. **Arbiter DelegationConfig** - Runtime configuration for the delegating arbiter
3. **Agent Registry** - Registry of available agents with skills/capabilities
4. **Delegating Arbiter** - LLM-driven arbiter that can delegate to specialized agents

### Important Distinction: Agents vs Capabilities

**Agents** (AgentRegistry) are **composite cognitive services** that can handle entire intents:
- Accept high-level goals and return plans or results
- Have skills, trust scores, and performance metrics
- Operate at intent granularity (dozens of agents)

**Capabilities** (CapabilityRegistry) are **atomic operations**:
- Execute single, deterministic functions
- Have schemas, permissions, and attestation
- Operate at call granularity (thousands of capabilities)

See [Spec 018 - Agent Registry & Discovery Layers](018-agent-registry-and-discovery-layers.md) for the complete architectural separation.

## Configuration Structure

### AgentConfig DelegationConfig

Located in `config/types.rs`, this is the primary configuration structure:

```rust
pub struct DelegationConfig {
    /// Whether delegation is enabled (fallback: enabled if agent registry present)
    pub enabled: Option<bool>,
    /// Score threshold to approve delegation (default 0.65)
    pub threshold: Option<f64>,
    /// Minimum number of matched skills required
    pub min_skill_hits: Option<u32>,
    /// Maximum number of candidate agents to shortlist
    pub max_candidates: Option<u32>,
    /// Weight applied to recent successful executions when updating scores
    pub feedback_success_weight: Option<f64>,
    /// Decay factor applied to historical success metrics
    pub feedback_decay: Option<f64>,
    /// Agent registry configuration
    pub agent_registry: Option<AgentRegistryConfig>,
    /// Adaptive threshold configuration for dynamic delegation decisions
    pub adaptive_threshold: Option<AdaptiveThresholdConfig>,
}

### Adaptive Threshold Configuration

The `AdaptiveThresholdConfig` structure provides dynamic threshold adjustment based on agent performance:

```rust
pub struct AdaptiveThresholdConfig {
    /// Whether adaptive threshold is enabled (default: true)
    pub enabled: Option<bool>,
    /// Base threshold value (default: 0.65)
    pub base_threshold: Option<f64>,
    /// Minimum threshold value (default: 0.3)
    pub min_threshold: Option<f64>,
    /// Maximum threshold value (default: 0.9)
    pub max_threshold: Option<f64>,
    /// Weight for success rate influence (default: 0.3)
    pub success_rate_weight: Option<f64>,
    /// Weight for historical performance (default: 0.2)
    pub historical_weight: Option<f64>,
    /// Decay factor for performance tracking (default: 0.95)
    pub decay_factor: Option<f64>,
    /// Minimum samples before adaptive threshold applies (default: 10)
    pub min_samples: Option<u32>,
    /// Environment variable override prefix (default: "CCOS_ADAPTIVE")
    pub env_prefix: Option<String>,
}
```

### Agent Registry Configuration

```rust
pub struct AgentRegistryConfig {
    /// Registry type (in_memory, database, etc.)
    pub registry_type: RegistryType,
    /// Database connection string (if applicable)
    pub database_url: Option<String>,
    /// Agent definitions
    pub agents: Vec<AgentDefinition>,
}

pub struct AgentDefinition {
    /// Unique agent identifier
    pub agent_id: String,
    /// Agent name/description
    pub name: String,
    /// Agent skills/capabilities (for matching, not execution)
    /// Note: These are skill tags for delegation matching, not executable capabilities
    pub capabilities: Vec<String>,
    /// Agent cost (per request)
    pub cost: f64,
    /// Agent trust score (0.0-1.0)
    pub trust_score: f64,
    /// Agent metadata
    pub metadata: HashMap<String, String>,
}
```

## Adaptive Threshold Functionality

The adaptive threshold system dynamically adjusts delegation decisions based on historical agent performance:

### Key Features
- **Decay-weighted Performance Tracking**: Historical success rates with configurable decay factors
- **Dynamic Threshold Calculation**: Thresholds adjust based on agent performance metrics
- **Bounds Enforcement**: Configurable minimum and maximum threshold values
- **Environment Variable Overrides**: Runtime configuration via environment variables
- **Minimum Samples Requirement**: Adaptive threshold only applies after sufficient performance data

### Configuration Example
```rust
let adaptive_config = AdaptiveThresholdConfig {
    enabled: Some(true),
    base_threshold: Some(0.65),
    min_threshold: Some(0.3),
    max_threshold: Some(0.9),
    success_rate_weight: Some(0.3),
    historical_weight: Some(0.2),
    decay_factor: Some(0.95),
    min_samples: Some(10),
    env_prefix: Some("CCOS_ADAPTIVE".to_string()),
};

agent_config.delegation.adaptive_threshold = Some(adaptive_config);
```

### Environment Variables
The adaptive threshold system supports environment variable overrides:
- `CCOS_ADAPTIVE_ENABLED`: Enable/disable adaptive threshold
- `CCOS_ADAPTIVE_BASE_THRESHOLD`: Set base threshold value
- `CCOS_ADAPTIVE_MIN_THRESHOLD`: Set minimum threshold value
- `CCOS_ADAPTIVE_MAX_THRESHOLD`: Set maximum threshold value
- `CCOS_ADAPTIVE_SUCCESS_RATE_WEIGHT`: Set success rate weight
- `CCOS_ADAPTIVE_HISTORICAL_WEIGHT`: Set historical performance weight
- `CCOS_ADAPTIVE_DECAY_FACTOR`: Set decay factor
- `CCOS_ADAPTIVE_MIN_SAMPLES`: Set minimum samples requirement

## Current Implementation vs Specification

### Implementation Status
The current implementation provides a **simplified foundation** for delegation:

- **`AgentDefinition`**: Basic structure with essential fields for Milestone 3
- **`AgentRegistryConfig`**: Simple in-memory registry configuration
- **Delegation Logic**: Basic skill matching and trust score evaluation

### Relationship to Spec 018
This implementation is **compatible with** but **simpler than** the full `AgentDescriptor` specification:

| Current Implementation | Spec 018 AgentDescriptor |
|------------------------|--------------------------|
| `agent_id` | `agent_id` |
| `name` | `name` (implied) |
| `capabilities` (skills) | `skills` |
| `cost` | `cost_model` (simplified) |
| `trust_score` | `trust_tier` (simplified) |
| `metadata` | Various fields (simplified) |
| ❌ Missing | `kind`, `supported_constraints`, `latency`, `historical_performance`, etc. |

### Migration Path
The current implementation can be **extended incrementally** to support the full specification:
1. Add new fields as optional (backward compatible)
2. Implement enhanced matching algorithms
3. Add governance and security features
4. Support advanced delegation scenarios

## Configuration Examples

```rust
let mut agent_config = AgentConfig::default();
agent_config.delegation.enabled = Some(true);
agent_config.delegation.threshold = Some(0.75);
agent_config.delegation.max_candidates = Some(5);
agent_config.delegation.min_skill_hits = Some(2);

let agent_registry = AgentRegistryConfig {
    registry_type: RegistryType::InMemory,
    database_url: None,
    agents: vec![
        AgentDefinition {
            agent_id: "sentiment_agent".to_string(),
            name: "Sentiment Analysis Agent".to_string(),
            capabilities: vec!["sentiment_analysis".to_string()],
            cost: 0.1,
            trust_score: 0.9,
            metadata: HashMap::new(),
        },
        AgentDefinition {
            agent_id: "optimization_agent".to_string(),
            name: "Performance Optimization Agent".to_string(),
            capabilities: vec!["performance_optimization".to_string()],
            cost: 0.2,
            trust_score: 0.8,
            metadata: HashMap::new(),
        }
    ],
};
agent_config.delegation.agent_registry = Some(agent_registry);
```

### RTFS Configuration Form

```clojure
(agent.config
  :version "0.1"
  :agent_id "my-agent"
  :profile "delegating"
  :delegation {
    :enabled true
    :threshold 0.75
    :max_candidates 5
    :min_skill_hits 2
    :agent_registry {
      :registry_type :in_memory
      :agents [
        {
          :agent_id "sentiment_agent"
          :name "Sentiment Analysis Agent"
          :capabilities ["sentiment_analysis" "text_processing"]
          :cost 0.1
          :trust_score 0.9
          :metadata {}
        }
        {
          :agent_id "optimization_agent"
          :name "Performance Optimization Agent"
          :capabilities ["performance_optimization" "monitoring"]
          :cost 0.2
          :trust_score 0.8
          :metadata {}
        }
      ]
    }
  }
  ;; ... other configuration
)
```

## Integration with CCOS

### Automatic Wiring

When CCOS initializes, it automatically:

1. Checks if delegation is enabled in the agent configuration
2. Converts the AgentConfig DelegationConfig to Arbiter DelegationConfig
3. Creates a DelegatingArbiter with the configured agent registry
4. Wires the delegating arbiter into the CCOS processing pipeline

### Conversion Process

The `to_arbiter_config()` method handles conversion from AgentConfig to Arbiter configuration:

```rust
impl DelegationConfig {
    pub fn to_arbiter_config(&self) -> crate::ccos::arbiter::arbiter_config::DelegationConfig {
        crate::ccos::arbiter::arbiter_config::DelegationConfig {
            enabled: self.enabled.unwrap_or(true),
            threshold: self.threshold.unwrap_or(0.65),
            max_candidates: self.max_candidates.unwrap_or(3) as usize,
            min_skill_hits: self.min_skill_hits.map(|hits| hits as usize),
            agent_registry: self.agent_registry.as_ref().map(|registry| {
                // Convert registry configuration
            }).unwrap_or_default(),
        }
    }
}
```

## Default Values

When configuration values are not specified, the following defaults are used:

- `enabled`: `true` (delegation enabled by default)
- `threshold`: `0.65` (65% confidence threshold)
- `max_candidates`: `3` (consider up to 3 agents)
- `min_skill_hits`: `None` (no minimum skill requirement)
- `agent_registry`: Empty in-memory registry

## Delegation Process

### Current Implementation (Milestone 3)
1. **Request Analysis**: The delegating arbiter analyzes the natural language request
2. **Skill Extraction**: Required skills are identified from the request
3. **Agent Matching**: Agents are matched based on skill overlap and trust scores
4. **Threshold Check**: Only agents above the threshold are considered
5. **Delegation Decision**: The best matching agent is selected for delegation
6. **Plan Generation**: A delegation plan is generated for the selected agent

### Example Delegation Flow
```
User Request: "Analyze the sentiment of customer feedback"

1. Skill Extraction: ["sentiment_analysis", "text_processing"]
2. Agent Matching: 
   - sentiment_agent: skills=["sentiment_analysis", "text_processing"], trust=0.9
   - general_agent: skills=["general"], trust=0.7
3. Selection: sentiment_agent (better skill match + higher trust)
4. Delegation: Send intent to sentiment_agent
5. Result: Agent returns plan or direct result
```

### Future Enhancement (Spec 018)
The delegation process will evolve to include:
- **Constraint Matching**: Budget, data locality, compliance requirements
- **Performance Prediction**: Latency and cost estimation
- **Multi-Agent Consideration**: Complex delegation scenarios
- **Governance Validation**: Trust tier vs intent sensitivity matrix

## Testing

Comprehensive tests are included to verify configuration functionality:

```bash
# Run delegation configuration tests
cargo test config::types::tests --lib

# Run all delegation-related tests
cargo test delegation --lib
```

## Environment Variables

The system supports environment variable overrides:

- `CCOS_DELEGATION_ENABLED`: Enable/disable delegation
- `CCOS_DELEGATION_THRESHOLD`: Set delegation threshold
- `CCOS_DELEGATION_MAX_CANDIDATES`: Set maximum candidates
- `CCOS_DELEGATION_MIN_SKILL_HITS`: Set minimum skill hits

## Best Practices

1. **Start with Conservative Thresholds**: Begin with higher thresholds (0.8+) and adjust based on performance
2. **Monitor Agent Performance**: Track success rates and adjust trust scores accordingly
3. **Use Descriptive Capabilities**: Define specific, well-defined capabilities for better matching
4. **Regular Registry Updates**: Keep the agent registry updated with current capabilities and performance metrics
5. **Test Thoroughly**: Use the provided test suite to verify configuration correctness

## Troubleshooting

### Common Issues

1. **No Agents Found**: Check that agent capabilities match the required capabilities
2. **Low Trust Scores**: Verify agent trust scores are above the threshold
3. **Configuration Not Applied**: Ensure the agent configuration is properly loaded
4. **Delegation Not Triggered**: Check that delegation is enabled and agents are available

### Debug Information

Enable debug logging to see delegation decisions:

```rust
// The system logs delegation decisions and agent selection
println!("Delegation threshold: {}", config.threshold);
println!("Available agents: {}", registry.agents.len());
println!("Selected agent: {}", selected_agent.agent_id);
```

## Implementation Status and Roadmap

### Current Implementation (Milestone 3)
The current `AgentDefinition` is a simplified version for initial delegation support:

- **Basic Fields**: `agent_id`, `name`, `capabilities` (skills), `cost`, `trust_score`, `metadata`
- **Purpose**: Enable delegation configuration and basic agent matching
- **Scope**: In-memory registry with simple skill-based matching

### Future Evolution (Post-Milestone 4)
The system will evolve toward the full `AgentDescriptor` specification from [Spec 018](018-agent-registry-and-discovery-layers.md):

**Planned Additional Fields:**
- `kind` (planner | analyzer | synthesizer | remote-arbiter | composite)
- `supported_constraints` (budget, data locality, compliance labels)
- `latency` (p50/p95 distribution)
- `historical_performance` (success rate, calibration metrics)
- `isolation_requirements` (network, data domains)
- `provenance` (signature, build hash)
- `quotas` (rate limits)

**Enhanced Features:**
- Embedding-based semantic retrieval
- Multi-agent consensus for high-risk intents
- Adaptive trust tier promotion/demotion
- Federated agent trust recalibration

## Future Enhancements

Planned improvements include:

1. **Adaptive Thresholds**: Dynamic threshold adjustment based on success rates
2. **Cost Optimization**: Consider cost in agent selection
3. **Load Balancing**: Distribute requests across multiple agents
4. **Health Monitoring**: Track agent health and availability
5. **Performance Metrics**: Detailed performance tracking and reporting
6. **Advanced Agent Descriptors**: Full implementation of Spec 018 AgentDescriptor
7. **Semantic Skill Matching**: Vector-based agent discovery
8. **Multi-Agent Orchestration**: Complex delegation scenarios
