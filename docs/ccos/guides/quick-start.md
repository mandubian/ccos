# Quick Start - Self-Learning Demo

Get the CCOS/RTFS self-learning demo running in 3 steps!

## Step 1: Set Up LLM Configuration

The demo requires an LLM to synthesize capabilities. Choose one option:

### Option A: Use OpenAI (Easiest)

```bash
export OPENAI_API_KEY="sk-your-api-key-here"
```

### Option B: Use Anthropic/Claude

```bash
export ANTHROPIC_API_KEY="sk-ant-your-api-key-here"
```

### Option C: Use OpenRouter

```bash
export OPENROUTER_API_KEY="sk-or-your-api-key-here"
```

## Step 2: Create Config File (if you don't have one)

```bash
# Create config directory
mkdir -p config

# Create minimal config
cat > config/agent_config.toml << 'EOF'
[llm_profiles]
default = "openai-fast"

[[llm_profiles.profiles]]
name = "openai-fast"
provider = "openai"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"

[[llm_profiles.profiles]]
name = "claude-fast"
provider = "anthropic"
model = "claude-3-5-sonnet-20241022"
api_key_env = "ANTHROPIC_API_KEY"

[[llm_profiles.profiles]]
name = "openrouter-free"
provider = "openrouter"
model = "meta-llama/llama-3.1-8b-instruct:free"
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
EOF
```

## Step 3: Run the Demo!

```bash
./demo_smart_assistant.sh full
```

That's it! 🎉

## What You'll See

```
🧠 CCOS/RTFS Self-Learning Demonstration 🧠
═══════════════════════════════════════════════════════════════

✓ CCOS initialized
✓ LLM: gpt-4o-mini via OpenAI

┌─────────────────────────────────────────────────────────────┐
│ PHASE 1: Initial Learning - Understanding Your Workflow    │
└─────────────────────────────────────────────────────────────┘

User Request: quantum computing applications in cryptography

💬 Interactive Preference Collection:
  Q1: What domains should I focus on?
  A1: academic papers, industry reports, expert blogs
  ...

┌─────────────────────────────────────────────────────────────┐
│ PHASE 2: Capability Synthesis (LLM-Driven)                 │
└─────────────────────────────────────────────────────────────┘

🔬 Analyzing interaction patterns with LLM...
✓ LLM analyzed conversation history
✓ Extracted parameter schema from interactions
✓ Generated RTFS capability definition

📦 Synthesized Capability:
```rtfs
(capability "research.smart-assistant.v1" ...)
```

[... more output ...]

═══════════════════════════════════════════════════════════════
                    LEARNING IMPACT ANALYSIS
═══════════════════════════════════════════════════════════════

┌─────────────────────┬───────────────┬───────────────┬──────────┐
│ Metric              │ Before Learn  │ After Learn   │ Gain     │
├─────────────────────┼───────────────┼───────────────┼──────────┤
│ Interaction Turns   │             6 │             1 │      6x  │
│ Questions Asked     │             5 │             0 │      -5  │
└─────────────────────┴───────────────┴───────────────┴──────────┘
```

## Troubleshooting

### Error: "Delegating arbiter not available"

**Quick Fix:**
```bash
# Make sure API key is set
export OPENAI_API_KEY="sk-..."

# Verify config exists
ls config/agent_config.toml

# Run with explicit config
./demo_smart_assistant.sh --config config/agent_config.toml full
```

### Error: "Failed to load config"

**Quick Fix:**
```bash
# Create config using Step 2 above
# Or copy example:
cp config/agent_config.example.toml config/agent_config.toml
```

### Want to customize?

```bash
# Try different research topics
export RESEARCH_TOPIC="neural architecture search"
./demo_smart_assistant.sh full

# Use different LLM
./demo_smart_assistant.sh --profile claude-fast full

# Enable interactive mode (type your own answers)
export CCOS_INTERACTIVE_ASK=1
./demo_smart_assistant.sh full

# Debug mode (see LLM prompts)
./demo_smart_assistant.sh --debug full
```

## Next Steps

1. ✅ Run the demo (you just did!)
2. 📚 Read [SELF_LEARNING_DEMO.md](SELF_LEARNING_DEMO.md) for concepts
3. 🔧 Read [DYNAMIC_DEMO_USAGE.md](DYNAMIC_DEMO_USAGE.md) for advanced usage
4. 🎨 Customize with different topics and preferences
5. 💻 Examine the generated RTFS code in `capabilities/generated/`

## Common Use Cases

### Research Different Topic
```bash
export RESEARCH_TOPIC="blockchain scalability solutions"
./demo_smart_assistant.sh full
```

### Use Free LLM (OpenRouter)
```bash
export OPENROUTER_API_KEY="sk-or-..."
./demo_smart_assistant.sh --profile openrouter-free full
```

### Customize Preferences
```bash
export CCOS_USER_ASK_RESPONSE_DOMAINS="github repositories, technical blogs"
export CCOS_USER_ASK_RESPONSE_DEPTH="code-level deep dive"
./demo_smart_assistant.sh full
```

### See What LLM Generates
```bash
export RTFS_SHOW_PROMPTS=1
./demo_smart_assistant.sh --debug full
```

---

**Need help?** Check the troubleshooting guides:
- [DYNAMIC_DEMO_USAGE.md](DYNAMIC_DEMO_USAGE.md#troubleshooting) - Detailed troubleshooting
- [SELF_LEARNING_DEMO.md](SELF_LEARNING_DEMO.md) - Conceptual overview









