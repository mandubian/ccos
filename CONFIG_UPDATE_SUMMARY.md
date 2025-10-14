# Configuration Update Summary

## Changes Made

Updated the self-learning demo to use the local configuration file `config/agent_config.toml` instead of requiring command-line arguments for LLM provider and model settings.

### Modified Files

1. **demo_self_learning.sh**
   - Now uses `--config ../config/agent_config.toml` for all modes
   - Removed `LLM_PROVIDER` and `LLM_MODEL` command-line parameters
   - Simplified usage: `./demo_self_learning.sh [mode]` (basic, full, persist)
   - Updated help text to explain configuration

2. **SELF_LEARNING_DEMO.md**
   - Updated examples to show simplified usage
   - Added configuration section explaining available LLM profiles
   - Shows both demo script and manual cargo usage

3. **README.md**
   - Simplified demo command examples
   - Removed provider/model arguments

### Benefits

1. **Simpler Usage**: No need to remember provider names or model IDs
   ```bash
   # Before:
   ./demo_self_learning.sh full openrouter meta-llama/llama-3.1-8b-instruct:free
   
   # After:
   ./demo_self_learning.sh full
   ```

2. **Centralized Configuration**: All LLM settings in one place
   - `config/agent_config.toml` contains profiles for:
     - OpenAI (gpt-4o-mini, gpt-4o)
     - Claude (claude-3-5-sonnet)
     - OpenRouter (free tier models)

3. **Consistent Behavior**: Same config used by example and interactive assistant

4. **Easy Profile Switching**: Can override via environment variable
   ```bash
   CCOS_LLM_PROFILE=claude-fast ./demo_self_learning.sh full
   ```

### Testing

The demo script compiles successfully and properly loads the configuration:
```
Mode: Basic Synthesis with Enhanced Visualization
Config: Using config/agent_config.toml
```

### Usage Examples

```bash
# Basic synthesis demo
./demo_self_learning.sh basic

# Full learning loop with proof-of-learning
./demo_self_learning.sh full

# With persistence
./demo_self_learning.sh persist

# Override to use Claude
CCOS_LLM_PROFILE=claude-fast ./demo_self_learning.sh full
```

### Configuration File Location

The config file at `config/agent_config.toml` includes:
- Default profile: `openai-fast` (gpt-4o-mini)
- Delegation enabled by default
- Debug flags for extracted intents/plans
- Multiple provider options

Users can edit this file to:
- Change default profile
- Add new profiles
- Adjust API keys (via environment variables)
- Configure model parameters
