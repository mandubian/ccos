# Blueprint Injection Architecture - Implementation Notes

**Status**: ✅ Implemented 2026-02-05  
**Location**: `ccos/src/cognitive_engine/delegating_engine.rs`

## Overview

The blueprint injection system automatically provides onboarding guidance to agents when they encounter non-operational skills.

## How It Works

1. **Detection**: `DelegatingCognitiveEngine::generate_delegated_plan` checks skill onboarding status from WorkingMemory
2. **Injection**: If skill is NOT `Operational`, injects formatted blueprint into `storable_intent.goal`
3. **Execution**: Agent follows the blueprint autonomously
4. **Completion**: Agent calls `ccos.skill.onboarding.mark_operational` to update state

## Blueprint Format

```markdown
MANDATORY SETUP REQUIRED

Step 1: setup-api-key (api_call)
- Operation: ccos.secrets.set
- Verification: (audit.succeeded? "ccos.secrets.set")

Step 2: verify-credentials (api_call)
- Operation: twitter-publisher.get-user-info
- Depends on: [setup-api-key]

Final: Call :ccos.skill.onboarding.mark_operational
```

## State Management

- **Storage**: Working Memory key `skill:{skill_id}:onboarding_state`
- **Type**: `SkillOnboardingState` (status, current_step, completed_steps, etc.)
- **Transitions**: Loaded →  NeedsSetup → PendingHumanAction → Operational

## Key Files

- `ccos/src/cognitive_engine/delegating_engine.rs` - Blueprint injection logic
- `ccos/src/skills/types.rs` - OnboardingStep, SkillOnboardingState
- `ccos/src/skills/mapper.rs` - Stores onboarding config in CapabilityManifest metadata
- `ccos/src/chat/predicate.rs` - Display trait for RTFS verification predicates
- `ccos/src/skills/onboarding_capabilities.rs` - mark_operational capability

## Testing

- Integration tests: `ccos/tests/onboarding_blueprint_integration_tests.rs`
- Sample skill: `capabilities/samples/twitter-publisher-skill.md`
- Test coverage: State transitions, predicate formatting, metadata storage

## Design Principles

- **Versatile**: Agent works with or without blueprints
- **Declarative**: Success criteria via RTFS predicates
- **Autonomous**: Agent is the "pilot", blueprint is the guide  
- **Persistent**: State survives restarts via WorkingMemory
