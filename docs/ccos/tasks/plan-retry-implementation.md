# Plan Generation Retry & Error Handling - Implementation Tasks

**Status:** Planned  
**Priority:** High (User Experience)  
**Estimated Effort:** 2-3 days  
**Dependencies:** None  
**Related Docs:** `docs/ccos/specs/021-plan-generation-retry.md`

## Overview

Implement robust retry mechanism for LLM-generated plans that fail to parse, with error feedback to help the LLM self-correct.

## Tasks

### Phase 1: Core Retry Infrastructure

- [ ] **Task 1.1**: Add `RetryConfig` struct to arbiter configuration
  - Location: `rtfs_compiler/src/ccos/arbiter/`
  - Fields: `max_retries`, `send_error_feedback`, `simplify_on_final_attempt`, `use_stub_fallback`
  - Default values: `max_retries=2`, `send_error_feedback=true`, `simplify_on_final_attempt=true`, `use_stub_fallback=false`
  - **Effort:** 1 hour

- [ ] **Task 1.2**: Implement `generate_plan_with_retry` wrapper function
  - Location: `rtfs_compiler/src/ccos/arbiter/llm_provider.rs` (OpenAI)
  - Location: `rtfs_compiler/src/ccos/arbiter/llm_provider.rs` (Anthropic)
  - Loop with attempt counter
  - Capture parse errors and broken plan text
  - **Effort:** 3-4 hours

- [ ] **Task 1.3**: Create `create_retry_prompt_with_feedback` helper
  - Generate feedback prompt with:
    - Original intent
    - Broken plan that failed
    - Parse error message with line/column
    - Common mistakes to avoid
    - Request for corrected plan
  - **Effort:** 2 hours

- [ ] **Task 1.4**: Implement `validate_and_parse_plan` validation step
  - Separate validation from plan generation
  - Return detailed parse errors (line, column, context)
  - Extract error messages that are helpful for LLM
  - **Effort:** 2 hours

### Phase 2: Logging & Observability

- [ ] **Task 2.1**: Add retry attempt logging
  - Log each attempt with: attempt number, intent, error (if any)
  - Use structured logging (tracing/log crate)
  - Log level: INFO for attempts, WARN for failures
  - **Effort:** 1 hour

- [ ] **Task 2.2**: Add metrics/counters for retry success rates
  - Track: total attempts, successful retries, failed retries
  - Optional: Export to prometheus/metrics system
  - Useful for monitoring LLM reliability
  - **Effort:** 2 hours (optional)

- [ ] **Task 2.3**: Improve error messages for end users
  - When all retries fail, provide:
    - Clear explanation of what went wrong
    - Suggestion to try simpler phrasing
    - Link to example patterns that work well
  - **Effort:** 1 hour

### Phase 3: Configuration & Environment

- [ ] **Task 3.1**: Add environment variable overrides
  - `CCOS_MAX_PLAN_RETRIES` - override max retry attempts
  - `CCOS_PLAN_RETRY_FEEDBACK` - enable/disable error feedback
  - `CCOS_PLAN_STUB_FALLBACK` - enable/disable stub fallback
  - **Effort:** 1 hour

- [ ] **Task 3.2**: Add retry config to AgentConfig JSON/TOML
  - Add `plan_retry` section to config schema
  - Example config snippet in docs
  - **Effort:** 1 hour

- [ ] **Task 3.3**: Document retry behavior in user guide
  - Add section to user documentation
  - Explain when retries happen
  - Show example of retry in action
  - Configuration options
  - **Effort:** 1-2 hours

### Phase 4: Advanced Features

- [ ] **Task 4.1**: Implement circuit breaker pattern
  - Stop retrying if consecutive failures > threshold (e.g., 5)
  - Prevents API cost explosion on systemic issues
  - Auto-reset after cooldown period
  - **Effort:** 3 hours

- [ ] **Task 4.2**: Add simplification logic for final retry
  - On last attempt, simplify the request:
    - Remove optional constraints
    - Use simpler language
    - Fall back to basic patterns
  - **Effort:** 2-3 hours

- [ ] **Task 4.3**: Implement stub/template fallback
  - When all retries fail, generate basic stub plan
  - Use template-based generation for known patterns
  - At least provides *something* that works
  - **Effort:** 3-4 hours

- [ ] **Task 4.4**: Cache successful fix patterns
  - When retry succeeds, cache the error → fix pattern
  - Include similar fixes in future initial prompts
  - Helps prevent repeat mistakes
  - **Effort:** 4-5 hours (optional)

### Phase 5: Testing

- [ ] **Task 5.1**: Unit tests for retry logic
  - Mock LLM responses (good, bad, then good)
  - Verify retry count
  - Verify error feedback is sent correctly
  - **Effort:** 3-4 hours

- [ ] **Task 5.2**: Integration tests with real LLM
  - Test with intentionally ambiguous prompts
  - Verify retry improves success rate
  - Test circuit breaker behavior
  - **Effort:** 2-3 hours

- [ ] **Task 5.3**: Test error message clarity
  - Manual testing of user-facing errors
  - Ensure helpful, not cryptic
  - Include examples of valid patterns
  - **Effort:** 1 hour

### Phase 6: Documentation & Examples

- [ ] **Task 6.1**: Update example code
  - Add retry config to `live_interactive_assistant.rs`
  - Show how to configure in `user_interaction_basic.rs`
  - **Effort:** 1 hour

- [ ] **Task 6.2**: Create troubleshooting guide
  - Common parse errors and fixes
  - How to interpret retry logs
  - When to increase retry count
  - **Effort:** 2 hours

- [ ] **Task 6.3**: Add metrics dashboard example
  - Optional: Grafana dashboard JSON for retry metrics
  - Useful for production monitoring
  - **Effort:** 2 hours (optional)

## Success Criteria

✅ **User Experience**
- Users rarely see parse errors (>90% success after retries)
- Clear, helpful error messages when all retries fail
- Minimal latency impact (retries only on failures)

✅ **Reliability**
- Retry success rate >60% (retry fixes >60% of initial failures)
- Circuit breaker prevents cost explosions
- Graceful degradation with stub fallback

✅ **Observability**
- All retry attempts logged
- Metrics show retry patterns
- Easy to debug when issues occur

✅ **Configuration**
- Easy to tune for different environments (dev/prod/cost-conscious)
- Environment variables work
- Sane defaults require no config

## Milestones

1. **M1 - Basic Retry (1 week)**: Phase 1 + Phase 2.1 + Phase 5.1
   - Core retry logic working
   - Basic logging
   - Unit tests pass
   
2. **M2 - Production Ready (2 weeks)**: M1 + Phase 2 + Phase 3
   - Full observability
   - Configurable
   - Documented
   
3. **M3 - Advanced Features (3+ weeks)**: M2 + Phase 4
   - Circuit breaker
   - Smart fallbacks
   - Pattern learning

## Non-Goals (Out of Scope)

- ❌ Retry for network/API errors (handle separately)
- ❌ Retry for semantic errors (plan runs but does wrong thing)
- ❌ User-initiated retry UI (separate feature)
- ❌ Cross-session learning (future enhancement)

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Increased API costs | Circuit breaker, low default max_retries (2) |
| Latency spikes | Only retry on parse failures (not every request) |
| Infinite loops | Hard limit on retries, timeout per attempt |
| Unhelpful feedback | Test error messages, iterate on prompts |

## Testing Plan

### Manual Testing Scenarios

1. **Scenario 1**: Ambiguous prompt → Parse error → Successful retry
2. **Scenario 2**: Very complex prompt → Multiple retries → Success
3. **Scenario 3**: Impossible request → All retries fail → Clear error
4. **Scenario 4**: High failure rate → Circuit breaker activates

### Automated Testing

- Unit tests: 15-20 test cases
- Integration tests: 5-10 scenarios with real LLM
- Load tests: Verify no performance regression

## Reference Implementation (Pseudocode)

```rust
async fn generate_plan_with_retry(
    &self,
    intent: &StorableIntent,
    config: &RetryConfig,
) -> Result<Plan, RuntimeError> {
    let mut last_error = None;
    let mut last_plan_text = None;
    
    for attempt in 1..=config.max_retries {
        let prompt = if attempt == 1 {
            self.create_initial_prompt(intent)
        } else if config.send_error_feedback {
            self.create_retry_prompt_with_feedback(
                intent,
                last_error.as_ref().unwrap(),
                last_plan_text.as_ref().unwrap(),
                attempt == config.max_retries
            )
        } else {
            self.create_initial_prompt(intent) // No feedback
        };
        
        let response = self.make_llm_request(prompt).await?;
        
        match self.parse_and_validate_plan(&response, intent) {
            Ok(plan) => {
                if attempt > 1 {
                    log::info!("✅ Plan retry succeeded on attempt {}", attempt);
                }
                return Ok(plan);
            }
            Err(e) => {
                log::warn!("❌ Attempt {}/{} failed: {}", attempt, config.max_retries, e);
                last_error = Some(e.to_string());
                last_plan_text = Some(response.clone());
                
                if attempt < config.max_retries {
                    continue; // Retry
                }
            }
        }
    }
    
    // All retries exhausted
    if config.use_stub_fallback {
        log::warn!("⚠️  Using stub fallback after {} failed attempts", config.max_retries);
        return Ok(self.generate_stub_plan(intent));
    }
    
    Err(RuntimeError::PlanGenerationFailed {
        attempts: config.max_retries,
        last_error: last_error.unwrap(),
    })
}
```

## Progress Tracking

- **Phase 1**: ⬜ Not Started (0/4 tasks)
- **Phase 2**: ⬜ Not Started (0/3 tasks)
- **Phase 3**: ⬜ Not Started (0/3 tasks)
- **Phase 4**: ⬜ Not Started (0/4 tasks)
- **Phase 5**: ⬜ Not Started (0/3 tasks)
- **Phase 6**: ⬜ Not Started (0/3 tasks)

**Overall**: 0/20 core tasks completed

---

**Last Updated**: 2025-09-29  
**Assigned To**: TBD  
**Related Issues**: N/A  
**Related PRs**: N/A
