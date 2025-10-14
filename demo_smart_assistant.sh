#!/bin/bash
# Enhanced CCOS/RTFS Self-Learning Demo - Smart Research Assistant
# Run from the repository root

set -e

# Colors and formatting
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m' # No Color

# Unicode symbols
CHECK="${GREEN}✓${NC}"
CROSS="${RED}✗${NC}"
ARROW="${CYAN}→${NC}"
BRAIN="${MAGENTA}🧠${NC}"
STAR="${YELLOW}⭐${NC}"
ROCKET="${GREEN}🚀${NC}"
BOOK="${BLUE}📚${NC}"
LIGHT="${YELLOW}💡${NC}"

# Banner function
print_banner() {
    echo ""
    echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BOLD}${CYAN}       ${BRAIN} CCOS/RTFS Self-Learning Demonstration ${BRAIN}${NC}"
    echo -e "${BOLD}${CYAN}           Smart Research Assistant Example${NC}"
    echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${NC}"
    echo ""
}

# Section header
print_section() {
    echo ""
    echo -e "${BOLD}${BLUE}┌────────────────────────────────────────────────────────────┐${NC}"
    echo -e "${BOLD}${BLUE}│ $1${NC}"
    echo -e "${BOLD}${BLUE}└────────────────────────────────────────────────────────────┘${NC}"
    echo ""
}

# Info message
info() {
    echo -e "  ${ARROW} $1"
}

# Success message
success() {
    echo -e "  ${CHECK} ${GREEN}$1${NC}"
}

# Error message
error() {
    echo -e "  ${CROSS} ${RED}$1${NC}"
}

# Highlight message
highlight() {
    echo -e "  ${STAR} ${YELLOW}$1${NC}"
}

# Check if we're in the right directory
if [ ! -d "rtfs_compiler" ]; then
    error "Please run this script from the repository root"
    exit 1
fi

cd rtfs_compiler

# Parse command line arguments
MODE="full"
PROFILE=""
DEBUG=""
TOPIC=""

print_usage() {
    echo "Usage: $0 [OPTIONS] [MODE]"
    echo ""
    echo "Modes:"
    echo "  learn    - Show initial learning phase only"
    echo "  apply    - Apply learned capability (requires prior learning)"
    echo "  full     - Complete learning loop (default) ⭐"
    echo ""
    echo "Options:"
    echo "  --profile PROFILE    LLM profile to use (from config/agent_config.toml)"
    echo "  --debug              Show detailed prompts and responses"
    echo "  --topic \"TOPIC\"      Custom research topic"
    echo "  --help               Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0 full"
    echo "  $0 --profile claude-fast full"
    echo "  $0 --topic \"neural architecture search\" full"
    echo "  $0 --debug learn"
    echo ""
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            print_banner
            print_usage
            exit 0
            ;;
        --profile)
            PROFILE="$2"
            shift 2
            ;;
        --debug)
            DEBUG="--debug-prompts"
            shift
            ;;
        --topic)
            TOPIC="$2"
            shift 2
            ;;
        learn|apply|full)
            MODE="$1"
            shift
            ;;
        -*)
            error "Unknown option: $1"
            print_usage
            exit 1
            ;;
        *)
            MODE="$1"
            shift
            ;;
    esac
done

print_banner

print_section "Configuration"

# Check for config file
if [ ! -f "../config/agent_config.toml" ]; then
    error "Config file not found: config/agent_config.toml"
    info "Please create a config file with your LLM settings"
    exit 1
fi

success "Found config/agent_config.toml"

# Display selected profile
if [ -n "$PROFILE" ]; then
    info "Using profile: ${BOLD}$PROFILE${NC}"
else
    info "Using default profile from config"
fi

# Display topic if custom
if [ -n "$TOPIC" ]; then
    highlight "Custom research topic: \"$TOPIC\""
    export RESEARCH_TOPIC="$TOPIC"
fi

# Display mode
info "Mode: ${BOLD}$MODE${NC}"

if [ -n "$DEBUG" ]; then
    highlight "Debug mode enabled"
fi

# Build command
CMD="cargo run --release --example user_interaction_smart_assistant -- \
    --config ../config/agent_config.toml \
    --mode $MODE"

if [ -n "$PROFILE" ]; then
    CMD="$CMD --profile $PROFILE"
fi

if [ -n "$DEBUG" ]; then
    CMD="$CMD $DEBUG"
fi

# Show what we're about to run
print_section "Executing Demo"

case "$MODE" in
    "learn")
        info "Phase 1: System learns from initial interaction"
        info "Phase 2: Capability synthesis and registration"
        info ""
        info "Expected: Multi-turn conversation → Synthesized capability"
        ;;
    "apply")
        info "Phase: Apply previously learned capability"
        info ""
        info "Expected: Direct capability invocation (no repeated questions)"
        highlight "Note: Requires prior 'learn' or 'full' run"
        ;;
    "full")
        info "Phase 1: Learn from interaction"
        info "Phase 2: Synthesize capability"
        info "Phase 3: Apply learned capability"
        info "Phase 4: Compare efficiency metrics"
        info ""
        success "This demonstrates the complete learning loop! ${ROCKET}"
        ;;
esac

echo ""
echo -e "${DIM}Running: $CMD${NC}"
echo ""

# Add a small delay for dramatic effect
sleep 1

# Run the demo
if eval "$CMD"; then
    echo ""
    print_section "Demo Completed Successfully! ${STAR}"
    
    # Show next steps based on mode
    case "$MODE" in
        "learn"|"full")
            echo ""
            success "Synthesized capability saved!"
            info "Check: ${BOLD}capabilities/generated/research.smart-assistant.v1.rtfs${NC}"
            echo ""
            highlight "What you can do next:"
            info "1. Run 'apply' mode to test the learned capability"
            info "2. Import the capability into your own RTFS programs"
            info "3. Try a different topic to see the capability generalize"
            info "4. Examine the generated RTFS code structure"
            echo ""
            info "Example:"
            echo -e "  ${DIM}./demo_smart_assistant.sh --topic \"distributed systems consensus\" full${NC}"
            ;;
        "apply")
            echo ""
            success "Capability application demonstrated!"
            echo ""
            highlight "Key observation:"
            info "No repeated questions - the system remembered your workflow!"
            echo ""
            ;;
    esac
    
    echo ""
    info "Read ${BOLD}SELF_LEARNING_DEMO.md${NC} for detailed explanation"
    info "Explore ${BOLD}rtfs_compiler/examples/user_interaction_smart_assistant.rs${NC} for implementation"
    echo ""
    
else
    echo ""
    print_section "Demo Failed ${CROSS}"
    error "An error occurred during execution"
    echo ""
    highlight "Troubleshooting:"
    info "1. Check your LLM API keys in environment variables"
    info "2. Verify config/agent_config.toml has valid LLM profiles"
    info "3. Try with --debug flag for more details"
    info "4. Check that required dependencies are installed"
    echo ""
    exit 1
fi

# Final banner
echo ""
echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BOLD}${GREEN}              Self-Learning Demonstration Complete!${NC}"
echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${NC}"
echo ""

