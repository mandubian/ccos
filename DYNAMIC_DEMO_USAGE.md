# Dynamic Self-Learning Demo Usage

The Smart Research Assistant demo is now **fully dynamic** - no more hardcoded simulations!

## What's Dynamic Now

### ✅ Real CCOS Execution
- Uses actual `ccos.user.ask` capability through RTFS plan execution
- Captures genuine conversation history
- Supports both interactive and canned response modes

### ✅ Real LLM Synthesis
- Calls delegating arbiter's `generate_raw_text()` with interaction history
- LLM analyzes actual conversation and generates RTFS capability
- Handles malformed responses and extracts valid RTFS code

### ✅ Real Capability Invocation
- Executes synthesized capability through CCOS plan validation
- Validates capability structure and parameters
- Proper error handling for missing dependencies

## Running the Dynamic Demo

### Basic Usage (Canned Responses)

```bash
# Uses environment variable responses for automation
./demo_smart_assistant.sh full
```

This mode sets `CCOS_USER_ASK_RESPONSE_*` environment variables with default answers, allowing the demo to run without manual input while still using real CCOS execution.

### Interactive Mode (Real User Input)

```bash
# Enable interactive prompts
export CCOS_INTERACTIVE_ASK=1
./demo_smart_assistant.sh full
```

In this mode, you'll be prompted to answer each question manually via stdin.

### Custom Canned Responses

```bash
# Override specific responses
export CCOS_USER_ASK_RESPONSE_DOMAINS="arxiv, research papers"
export CCOS_USER_ASK_RESPONSE_DEPTH="deep dive with code examples"
export CCOS_USER_ASK_RESPONSE_FORMAT="technical report with diagrams"
export CCOS_USER_ASK_RESPONSE_SOURCES="github, arxiv, papers with code"
export CCOS_USER_ASK_RESPONSE_TIME="1 week for thorough analysis"

./demo_smart_assistant.sh full
```

### With Real LLM Provider

```bash
# Ensure API key is set
export OPENAI_API_KEY="sk-..."
# or
export ANTHROPIC_API_KEY="sk-ant-..."
# or
export OPENROUTER_API_KEY="sk-or-..."

# Run with specific profile
./demo_smart_assistant.sh --profile claude-fast full
```

## What Happens Under the Hood

### Phase 1: Real Preference Gathering

```rust
// Executes this for each question:
let plan_body = format!("(call :ccos.user.ask \"{}\")", question);
let plan = Plan::new_rtfs(plan_body, vec![]);
let result = ccos.validate_and_execute_plan(plan, &ctx).await?;

// Captures actual response:
let answer = match &result.value {
    Value::String(s) => s.clone(),
    other => other.to_string(),
};
```

### Phase 2: Real LLM Synthesis

```rust
// Builds prompt from actual interaction history
let synthesis_prompt = format!(
    "Interaction History:\n{}\n\nGenerate RTFS capability...",
    interaction_summary
);

// Calls real LLM
let raw_response = arbiter.generate_raw_text(&synthesis_prompt).await?;

// Extracts and validates RTFS
let capability_spec = extract_capability_from_response(&raw_response)?;
```

### Phase 3: Real Capability Invocation

```rust
// Invokes through CCOS
let capability_invocation = format!(
    "(call :{} {{:topic \"{}\"}})",
    capability_id, new_topic
);
let plan = Plan::new_rtfs(capability_invocation, vec![]);
let result = ccos.validate_and_execute_plan(plan, &ctx).await?;
```

## Environment Variables

### Interaction Control

- `CCOS_INTERACTIVE_ASK=1` - Enable interactive stdin prompts
- `CCOS_USER_ASK_RESPONSE_DOMAINS` - Canned answer for domains question
- `CCOS_USER_ASK_RESPONSE_DEPTH` - Canned answer for depth question
- `CCOS_USER_ASK_RESPONSE_FORMAT` - Canned answer for format question
- `CCOS_USER_ASK_RESPONSE_SOURCES` - Canned answer for sources question
- `CCOS_USER_ASK_RESPONSE_TIME` - Canned answer for time question

### Research Topics

- `RESEARCH_TOPIC` - First research topic (learning phase)
- `SECOND_RESEARCH_TOPIC` - Second research topic (application phase)

### LLM Configuration

- `OPENAI_API_KEY` - OpenAI API key
- `ANTHROPIC_API_KEY` - Anthropic/Claude API key
- `OPENROUTER_API_KEY` - OpenRouter API key
- `CCOS_LLM_PROVIDER` - Provider override (openai|anthropic|openrouter|stub)
- `CCOS_LLM_MODEL` - Model override
- `RTFS_SHOW_PROMPTS=1` - Show LLM prompts and responses

## Examples

### Fully Automated Demo
```bash
./demo_smart_assistant.sh full
```

### Interactive Learning
```bash
export CCOS_INTERACTIVE_ASK=1
./demo_smart_assistant.sh learn
```

### Custom Research Workflow
```bash
export RESEARCH_TOPIC="distributed consensus algorithms"
export CCOS_USER_ASK_RESPONSE_DOMAINS="academic research, production systems"
export CCOS_USER_ASK_RESPONSE_DEPTH="detailed with implementation examples"
export CCOS_USER_ASK_RESPONSE_FORMAT="technical whitepaper"

./demo_smart_assistant.sh --profile openai-balanced full
```

### Debug Mode
```bash
export RTFS_SHOW_PROMPTS=1
./demo_smart_assistant.sh --debug full
```

## Expected Output

### With Real LLM

The LLM will generate unique RTFS capabilities based on your actual interaction:

```rtfs
(capability "research.smart-assistant.v1"
  :description "Analyzes academic papers and industry reports on distributed systems"
  :parameters {:topic "string"}
  :implementation
    (do
      (step "Gather Sources"
        (call :research.gather {:sources ["arxiv" "IEEE"] :topic topic}))
      (step "Deep Analysis"  
        (call :research.analyze {:depth "comprehensive" :examples true}))
      (step "Format Report"
        (call :research.format {:style "whitepaper" :citations true}))
      (step "Return"
        {:status "completed" :report formatted_output})))
```

### Error Handling

If sub-capabilities aren't registered, you'll see:

```
⚠ Capability execution error: Capability 'research.gather' not found
→ This is expected if the capability calls sub-capabilities not yet registered
✓ Capability invocation demonstrated (structure validated)
```

This is normal - the demo focuses on learning and synthesis, not implementing the full research stack.

## Troubleshooting

### ❌ "Delegating arbiter not available"

This is the most common error. The demo requires a working LLM configuration.

**Root Causes:**
1. No config file provided
2. Config file has no valid LLM profiles
3. Missing API key for the selected provider
4. Invalid provider/model combination

**Solutions:**

#### Option 1: Use Valid Config (Recommended)
```bash
# Make sure config exists and has llm_profiles
cat config/agent_config.toml

# Run with config
./demo_smart_assistant.sh --config config/agent_config.toml full
```

#### Option 2: Set API Key
```bash
# For OpenAI
export OPENAI_API_KEY="sk-..."
./demo_smart_assistant.sh --profile openai-fast full

# For Anthropic/Claude
export ANTHROPIC_API_KEY="sk-ant-..."
./demo_smart_assistant.sh --profile claude-fast full

# For OpenRouter
export OPENROUTER_API_KEY="sk-or-..."
./demo_smart_assistant.sh --profile openrouter-free full
```

#### Option 3: Verify Config Contents
```bash
# Check if config has llm_profiles section
grep -A 10 "llm_profiles" config/agent_config.toml

# Should see something like:
# [llm_profiles]
# default = "openai-fast"
# [[llm_profiles.profiles]]
# name = "openai-fast"
# provider = "openai"
# model = "gpt-4o-mini"
```

#### Option 4: Create Minimal Config
If you don't have a config file, create one:

```toml
# config/agent_config.toml
[llm_profiles]
default = "openai-fast"

[[llm_profiles.profiles]]
name = "openai-fast"
provider = "openai"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"
```

Then set your API key:
```bash
export OPENAI_API_KEY="sk-..."
./demo_smart_assistant.sh full
```

### "Failed to ask question"
- Check `ccos.user.ask` capability is registered
- Verify RuntimeContext allows the capability
- Enable debug mode to see full error

### LLM Returns Invalid RTFS
- The code has fallback extraction logic
- Check `RTFS_SHOW_PROMPTS=1` to see raw response
- Try a different LLM model/provider

## Next Steps

1. Run the demo: `./demo_smart_assistant.sh full`
2. Examine generated capability: `cat capabilities/generated/research.smart-assistant.v1.rtfs`
3. Try custom topics and preferences
4. Enable interactive mode for real conversations
5. Experiment with different LLM providers

## Differences from Previous Version

| Aspect | Old (Simulated) | New (Dynamic) |
|--------|----------------|---------------|
| User input | Hardcoded strings | Real `ccos.user.ask` execution |
| Capability synthesis | Template generation | LLM generates from conversation |
| Capability invocation | Fake sleep delays | Real CCOS plan execution |
| Interaction history | Predetermined | Captured from actual execution |
| LLM involvement | None | Full synthesis via arbiter |
| Customization | Source code edits | Environment variables |

---

**The demo now showcases genuine CCOS self-learning capabilities!** 🎉

