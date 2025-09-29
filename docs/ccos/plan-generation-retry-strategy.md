# Plan Generation Retry Strategy

## Problem

LLMs sometimes generate broken/invalid RTFS plans that fail to parse. Currently, these errors immediately propagate to the end user with no recovery attempt.

## Solution: Self-Healing Plan Generation

### Strategy

```
Attempt 1: Generate plan
  ↓
Parse & validate
  ↓
✗ FAIL → Send error feedback to LLM
  ↓
Attempt 2: Regenerate with error context
  ↓
Parse & validate
  ↓
✗ FAIL → Send detailed error + examples
  ↓
Attempt 3: Final attempt with simplified request
  ↓
✓ SUCCESS or ✗ User-facing error
```

### Implementation Approach

#### 1. **Retry with Error Feedback** (Recommended)

When parsing fails, send the broken plan back to the LLM with:
- The parse error message
- The exact line/position of the error
- A correct example similar to what was attempted

**Example feedback prompt:**
```
The plan you generated failed to parse with error:
  ParseError at line 3: "expected expression after 'let' bindings"

Your generated plan:
(do
  (step "Bad" (let [name (call :ccos.user.ask "Name?")]))
)

The problem: 'let' requires a body expression after bindings.

Correct pattern:
(step "Good" 
  (let [name (call :ccos.user.ask "Name?")]
    (call :ccos.echo {:message name})))

Please regenerate the plan fixing this error.
```

#### 2. **Configuration**

```rust
pub struct RetryConfig {
    /// Maximum retry attempts (recommended: 2-3)
    pub max_retries: usize,
    
    /// Whether to send detailed error feedback to LLM
    pub send_error_feedback: bool,
    
    /// Whether to simplify request on final retry
    pub simplify_on_final_attempt: bool,
    
    /// Fallback to template/stub on all failures
    pub use_stub_fallback: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            send_error_feedback: true,
            simplify_on_final_attempt: true,
            use_stub_fallback: false,
        }
    }
}
```

#### 3. **Retry Logic Flow**

```rust
async fn generate_plan_with_retry(
    &self,
    intent: &StorableIntent,
    context: Option<HashMap<String, String>>,
) -> Result<Plan, RuntimeError> {
    let config = RetryConfig::default();
    let mut last_error: Option<String> = None;
    let mut last_plan: Option<String> = None;
    
    for attempt in 1..=config.max_retries {
        // Generate plan (with error feedback if this is a retry)
        let result = if attempt == 1 {
            self.generate_plan_initial(intent, context.clone()).await
        } else {
            self.generate_plan_with_feedback(
                intent,
                context.clone(),
                last_error.as_deref(),
                last_plan.as_deref(),
                attempt == config.max_retries, // is_final_attempt
            ).await
        };
        
        match result {
            Ok(plan) => {
                // Validate plan parses correctly
                match self.validate_and_parse_plan(&plan) {
                    Ok(validated_plan) => {
                        if attempt > 1 {
                            eprintln!("[retry] ✅ Plan generated successfully on attempt {}", attempt);
                        }
                        return Ok(validated_plan);
                    }
                    Err(parse_error) => {
                        eprintln!("[retry] ❌ Attempt {}/{} failed: {}", 
                                  attempt, config.max_retries, parse_error);
                        last_error = Some(parse_error.to_string());
                        last_plan = Some(format!("{:?}", plan));
                        
                        if attempt == config.max_retries {
                            break; // Exit loop to return error
                        }
                        // Continue to next retry
                    }
                }
            }
            Err(e) => {
                return Err(e); // Network/API errors don't retry
            }
        }
    }
    
    // All retries exhausted
    if config.use_stub_fallback {
        eprintln!("[retry] ⚠️  All retries failed, using stub fallback");
        return Ok(self.generate_stub_plan(intent));
    }
    
    Err(RuntimeError::Generic(format!(
        "Failed to generate valid plan after {} attempts. Last error: {}",
        config.max_retries,
        last_error.unwrap_or_else(|| "Unknown error".to_string())
    )))
}
```

#### 4. **Error Feedback Prompt Template**

```rust
fn create_retry_prompt_with_feedback(
    intent: &StorableIntent,
    parse_error: &str,
    broken_plan: &str,
    attempt: usize,
) -> String {
    format!(
        r#"Previous attempt (#{}) to generate a plan failed to parse.

Original intent: {}

Your previous plan:
```
{}
```

Parse error:
{}

Common issues to avoid:
- let bindings must have a body expression: (let [x val] <body>)
- Variables don't cross step boundaries - keep them in same step
- match requires pattern-result pairs: (match val pat1 res1 pat2 res2 _ default)
- All capability IDs must start with colon: :ccos.echo not ccos.echo

Please generate a corrected plan that fixes this error."#,
        attempt,
        intent.goal,
        broken_plan,
        parse_error
    )
}
```

### Benefits

✅ **Self-healing**: LLM can fix its own mistakes
✅ **Better UX**: Users get working plans instead of cryptic errors
✅ **Learning**: Error feedback helps LLM avoid repeat mistakes
✅ **Cost-effective**: 2-3 retries is manageable cost vs user frustration
✅ **Transparent**: Log attempts for debugging

### Costs/Tradeoffs

❌ **Latency**: Retries add 2-6 seconds per failure
❌ **API costs**: Extra LLM calls (but only on failures)
❌ **Complexity**: More code paths to test

### Recommended Settings

**Development/Testing:**
```rust
RetryConfig {
    max_retries: 3,
    send_error_feedback: true,
    simplify_on_final_attempt: true,
    use_stub_fallback: false, // See real errors
}
```

**Production:**
```rust
RetryConfig {
    max_retries: 2,
    send_error_feedback: true,
    simplify_on_final_attempt: true,
    use_stub_fallback: true, // Graceful degradation
}
```

**Cost-conscious:**
```rust
RetryConfig {
    max_retries: 1,
    send_error_feedback: true,
    simplify_on_final_attempt: false,
    use_stub_fallback: true,
}
```

## Alternative: Template Fallback Only

For simpler needs:
1. Try LLM plan generation once
2. On failure → Use template-based plan for known patterns
3. Unknown patterns → Clear error to user

**Pros**: Simple, fast, predictable
**Cons**: Less flexible, doesn't help LLM improve

## Recommendation

**Implement retry-with-feedback** for user-facing applications where robustness matters more than latency. The self-healing aspect significantly improves UX and actually helps the LLM learn from its mistakes in the same session.

Configure `max_retries=2` as a reasonable balance between reliability and latency/cost.

## Implementation Checklist

- [ ] Add `RetryConfig` struct to arbiter configuration
- [ ] Implement `generate_plan_with_retry` wrapper
- [ ] Create `create_retry_prompt_with_feedback` helper
- [ ] Add retry attempt logging/metrics
- [ ] Update tests to verify retry logic
- [ ] Document retry behavior in user guide
- [ ] Add environment variable overrides (CCOS_MAX_PLAN_RETRIES)
- [ ] Implement circuit breaker (stop retrying if consecutive failures > N)

## Example User Experience

### Without Retry:
```
❌ Error: Failed to parse plan JSON: missing field 'steps'
```
**User reaction**: Frustrated, retries request manually

### With Retry:
```
[Attempt 1] Generating plan...
[Attempt 1] ❌ Parse error, retrying with feedback...
[Attempt 2] ✅ Plan generated successfully
✅ Result: Success!
```
**User reaction**: Happy, didn't even notice the issue!

## Future Enhancements

1. **Pattern learning**: Cache common fix patterns to include in initial prompts
2. **Adaptive retries**: Adjust max_retries based on model/task complexity
3. **Metrics dashboard**: Track retry rates by model/intent type
4. **User feedback loop**: Let users report "this plan is wrong" to trigger regeneration
