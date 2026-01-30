# Chat Mode Audit Events (Phase 0)

## Purpose
Define the minimal audit events required to make chat mode traceable and policy-compliant.

## Event Types (Required)
1. **message.ingest**
   - When an inbound message is accepted
   - Fields (event-specific): message_id, channel_id, sender_id, message_timestamp, classifications

2. **quarantine.access**
   - When a transform capability reads quarantined data
   - Fields (event-specific): pointer_id, capability_id, justification, policy_decision

3. **transform.output**
   - When transform returns structured output
   - Fields (event-specific): capability_id, output_schema, output_classification

4. **policy.decision**
   - When a policy gate allows/denies an action
   - Fields (event-specific): gate, decision, rule_id, reason

5. **egress.attempt**
   - When outbound send is attempted
   - Fields (event-specific): channel_id, payload_classification, decision

6. **approval.request**
   - When an approval is requested
   - Fields (event-specific): approval_id, type, scope, requester

7. **approval.decision**
   - When an approval is approved/denied/modified
   - Fields (event-specific): approval_id, decision, reviewer, changes

## Required Properties
### Common event envelope (required for all events)
Every event MUST include:
- `event_id`
- `event_type`
- `timestamp`
- `session_id`
- `run_id`
- `step_id` (may be a synthetic step for non-capability actions, e.g. `message.ingest`)
- `policy_pack_version`

All events are recorded in the causal chain.

### Capability provenance (required when a capability is involved)
For events that directly involve a capability execution or capability-mediated access, also include:
- `capability_id`
- `capability_version`

### Policy rule provenance (required when a policy decision is made)
For events that represent (or are the direct consequence of) a policy gate decision, also include:
- `rule_id`

## Alignment
- Causal chain: `docs/ccos/specs/003-causal-chain.md`
- Chat mode policy pack: `docs/ccos/specs/038-chat-mode-policy-pack.md`
