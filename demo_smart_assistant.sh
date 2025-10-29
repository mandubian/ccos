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
PROFILE=""
DEBUG_PROMPTS=0
GOAL=""

print_usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --goal \"TEXT\"       Natural-language goal (otherwise prompts interactively)"
    echo "  --profile PROFILE    LLM profile to use (from config/agent_config.toml)"
    echo "  --debug              Show detailed prompts and responses"
    echo "  --topic \"TEXT\"      Backwards-compatible alias for --goal"
    echo "  --help               Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0"
    echo "  $0 --goal \"plan a weekend trip to Paris\""
    echo "  $0 --profile openrouter_free:fast"
    echo "  $0 --debug --goal \"research retrieval augmented generation\""
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
            DEBUG_PROMPTS=1
            shift
            ;;
        --goal)
            GOAL="$2"
            shift 2
            ;;
        --topic)
            GOAL="$2"
            shift 2
            ;;
        -*)
            error "Unknown option: $1"
            print_usage
            exit 1
            ;;
        *)
            if [ -z "$GOAL" ]; then
                GOAL="$1"
            else
                error "Unrecognized argument: $1"
                print_usage
                exit 1
            fi
            shift
            ;;
    esac
done

print_banner

print_section "Configuration"

# Check for config file
if [ ! -f "config/agent_config.toml" ]; then
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

# Display goal configuration
if [ -n "$GOAL" ]; then
    highlight "Goal override: \"$GOAL\""
else
    info "Goal: will prompt interactively"
fi

# Display debug flag if requested
if [ "$DEBUG_PROMPTS" -eq 1 ]; then
    highlight "Debug prompts enabled"
fi

# Build command (use array to preserve spacing)
CMD=(cargo run --release --example smart_assistant_demo --
    --config config/agent_config.toml)

if [ -n "$PROFILE" ]; then
    CMD+=(--profile "$PROFILE")
fi

if [ -n "$GOAL" ]; then
    CMD+=(--goal "$GOAL")
fi

if [ "$DEBUG_PROMPTS" -eq 1 ]; then
    CMD+=(--debug-prompts)
fi

# Show what we're about to run
print_section "Executing Demo"

info "Phase 1: Delegating arbiter captures intent and clarifications"
info "Phase 2: LLM drafts plan skeleton with metadata instrumentation"
info "Phase 3: Stub capabilities register so the plan can execute end-to-end"
info ""
success "Expect synthesized artifacts under capabilities/generated/ ${ROCKET}"

echo ""
CMD_STR=$(printf "%q " "${CMD[@]}")
echo -e "${DIM}Running: ${CMD_STR}${NC}"
echo ""

# Add a small delay for dramatic effect
sleep 1

# Run the demo
if "${CMD[@]}"; then
    echo ""
    print_section "Demo Completed Successfully! ${STAR}"
    
    highlight "Artifacts to inspect:"
    info "1. capabilities/generated/ for synthesized capabilities and plans"
    info "2. demo logs for LLM metadata in the intent graph"
    info "3. config/agent_config.toml to adjust model routing"
    echo ""
    highlight "What you can try next:"
    info "1. Pass --goal to script to skip interactive prompt"
    info "2. Switch --profile to explore other OpenRouter models"
    info "3. Re-run with --debug to inspect raw prompts"
    
    echo ""
    info "Read ${BOLD}SELF_LEARNING_DEMO.md${NC} for detailed explanation"
    info "Explore ${BOLD}rtfs_compiler/examples/smart_assistant_demo.rs${NC} for implementation"
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

