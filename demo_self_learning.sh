#!/bin/bash
# Demo script for CCOS/RTFS Self-Learning Capabilities
# Run from the repository root

set -e

echo "========================================="
echo "CCOS/RTFS Self-Learning Capability Demo"
echo "========================================="
echo ""

# Check if we're in the right directory
if [ ! -d "rtfs_compiler" ]; then
    echo "Error: Please run this script from the repository root"
    exit 1
fi

cd rtfs_compiler

# Parse command line arguments
ENABLE_DELEGATION=false
MODE="basic"

while [[ $# -gt 0 ]]; do
    case $1 in
        --enable-delegation)
            ENABLE_DELEGATION=true
            shift
            ;;
        --mode)
            MODE="$2"
            shift
            shift
            ;;
        -*)
            echo "Unknown option: $1"
            echo "Usage: $0 [--enable-delegation] [--mode MODE]"
            exit 1
            ;;
        *)
            MODE="$1"
            shift
            ;;
    esac
done

case "$MODE" in
    "basic")
        echo "Mode: Basic Synthesis with Enhanced Visualization"
        echo "Config: Using config/agent_config.toml"
        if [ "$ENABLE_DELEGATION" = true ]; then
            echo "Delegation: Enabled"
        else
            echo "Delegation: Disabled"
        fi
        echo ""
        echo "This will show:"
        echo "  ✓ Learning baseline (initial capabilities)"
        echo "  ✓ Multi-turn interaction"
        echo "  ✓ Capability synthesis"
        echo "  ✓ Learning outcome (efficiency gains)"
        echo ""
        
        CMD="cargo run --example user_interaction_progressive_graph -- \
            --config ../config/agent_config.toml"
        if [ "$ENABLE_DELEGATION" = true ]; then
            CMD="$CMD --enable-delegation"
        fi
        CMD="$CMD --synthesize-capability"
        eval "$CMD"
        ;;
    
    "full")
        echo "Mode: Full Learning Loop with Proof-of-Learning"
        echo "Config: Using config/agent_config.toml"
        if [ "$ENABLE_DELEGATION" = true ]; then
            echo "Delegation: Enabled"
        else
            echo "Delegation: Disabled"
        fi
        echo ""
        echo "This will show:"
        echo "  ✓ Learning baseline"
        echo "  ✓ Multi-turn interaction"
        echo "  ✓ Capability synthesis"
        echo "  ✓ Learning outcome"
        echo "  ✓ Proof-of-learning test"
        echo ""
        
        CMD="cargo run --example user_interaction_progressive_graph -- \
            --config ../config/agent_config.toml"
        if [ "$ENABLE_DELEGATION" = true ]; then
            CMD="$CMD --enable-delegation"
        fi
        CMD="$CMD --synthesize-capability \
            --demo-learning-loop \
            --persist-synthesized"
        eval "$CMD"
        ;;
    
    "persist")
        echo "Mode: Synthesis with Persistence"
        echo "Config: Using config/agent_config.toml"
        if [ "$ENABLE_DELEGATION" = true ]; then
            echo "Delegation: Enabled"
        else
            echo "Delegation: Disabled"
        fi
        echo ""
        echo "Synthesized capabilities will be saved to:"
        echo "  → generated_capabilities/*.rtfs"
        echo ""
        
        CMD="cargo run --example user_interaction_progressive_graph -- \
            --config ../config/agent_config.toml"
        if [ "$ENABLE_DELEGATION" = true ]; then
            CMD="$CMD --enable-delegation"
        fi
        CMD="$CMD --synthesize-capability \
            --persist-synthesized"
        eval "$CMD"
        ;;
    
    *)
        echo "Usage: $0 [--enable-delegation] [--mode MODE] [MODE]"
        echo ""
        echo "Modes:"
        echo "  basic    - Basic synthesis with visualization (default)"
        echo "  full     - Full learning loop with proof-of-learning"
        echo "  persist  - Synthesis with disk persistence"
        echo ""
        echo "Options:"
        echo "  --enable-delegation    Enable LLM delegation for capability synthesis"
        echo "  --mode MODE            Set the demo mode (same as positional argument)"
        echo ""
        echo "Configuration:"
        echo "  Uses config/agent_config.toml for LLM settings"
        echo "  Default profile: openai-fast (gpt-4o-mini)"
        echo "  Alternative profiles: openai-balanced, claude-fast, openrouter-free"
        echo ""
        echo "To override profile, set environment variable:"
        echo "  CCOS_LLM_PROFILE=openrouter-free $0 --enable-delegation full"
        echo ""
        echo "Examples:"
        echo "  $0 basic"
        echo "  $0 --enable-delegation full"
        echo "  $0 --enable-delegation --mode persist"
        echo "  CCOS_LLM_PROFILE=claude-fast $0 --enable-delegation full"
        exit 1
        ;;
esac

echo ""
echo "========================================="
echo "Demo completed!"
echo "========================================="
echo ""
echo "Next steps:"
echo "  • Review the output above"
echo "  • Check generated_capabilities/ for persisted specs (if --persist-synthesized was used)"
echo "  • Read SELF_LEARNING_DEMO.md for detailed explanation"
echo ""
