#!/bin/bash
# Demo script for CCOS Gateway with Real-Time Monitoring
# Starts the gateway and gateway monitor
# Run ccos-chat separately in another terminal

set -e

# Disable proxy for localhost connections
# export no_proxy="localhost,127.0.0.1,${no_proxy}"
# export NO_PROXY="localhost,127.0.0.1,${NO_PROXY}"

echo "🚀 CCOS Gateway + Monitor Demo"
echo "==============================="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check if we're in the right directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

export CCOS_QUARANTINE_KEY="YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE="
export CCOS_GATEWAY_SPAWN_AGENTS=1
export CCOS_AGENT_BINARY="$SCRIPT_DIR/../target/debug/ccos-agent"

# Agent configuration
export CCOS_AGENT_CONFIG_PATH="$SCRIPT_DIR/../config/agent_config.toml"
export CCOS_AGENT_ENABLE_LLM=true

# Pass API keys through (will be picked up by agent config logic)
export GEMINI_API_KEY=${GEMINI_API_KEY}
export OPENROUTER_API_KEY=${OPENROUTER_API_KEY}
export RUST_LOG=ccos_agent=debug
# LLM HTTP timeout for iterative consult calls (can exceed 60s on free/shared models)
export CCOS_LLM_TIMEOUT_SECS=${CCOS_LLM_TIMEOUT_SECS:-180}

# Monitor authentication (must match gateway --admin-tokens)
DEMO_ADMIN_TOKEN="${CCOS_DEMO_ADMIN_TOKEN:-admin-token}"

# Build the binaries first
echo -e "${BLUE}Building binaries...${NC}"
# cargo build --bin ccos-chat-gateway --bin ccos-gateway-monitor --bin ccos-agent --release 2>&1 | tail -10
cargo build --bin ccos-chat-gateway --bin ccos-gateway-monitor --bin ccos-agent 2>&1 | tail -10
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# Configuration
GATEWAY_PORT=8822
GATEWAY_URL="http://127.0.0.1:${GATEWAY_PORT}"

echo -e "${YELLOW}Configuration:${NC}"
echo "  Gateway URL: ${GATEWAY_URL}"
echo "  Config file: config/agent_config.toml"
echo "  LLM Enabled: ${CCOS_AGENT_ENABLE_LLM}"
echo ""

# Function to cleanup processes on exit
cleanup() {
    echo ""
    echo -e "${YELLOW}Shutting down...${NC}"
    # Kill spawned agents by binary name
    pkill -f ccos-agent 2>/dev/null || true
    echo "  ✓ Agents stopped"
    if [ -n "$GATEWAY_PID" ]; then
        kill $GATEWAY_PID 2>/dev/null || true
        echo "  ✓ Gateway stopped"
    fi
    if [ -n "$MONITOR_PID" ]; then
        kill $MONITOR_PID 2>/dev/null || true
        echo "  ✓ Monitor stopped"
    fi
    # Kill by port as fallback
    for port in 8822 8833; do
        fuser -k -n tcp $port 2>/dev/null || true
    done
    echo -e "${GREEN}Demo complete!${NC}"
}
trap cleanup EXIT INT TERM


# Start the Gateway
echo -e "${BLUE}Starting Gateway on port ${GATEWAY_PORT}...${NC}"
RUST_LOG=ccos=debug "$SCRIPT_DIR/../target/debug/ccos-chat-gateway" serve \
    --bind-addr 127.0.0.1:${GATEWAY_PORT} \
    --connector-bind-addr 127.0.0.1:8833 \
    --connector-secret "demo-secret" \
    --admin-tokens "${DEMO_ADMIN_TOKEN}" \
    --allow-senders "user1" \
    --allow-channels "general" \
    --http-allow-hosts api.coingecko.com \
    --min-send-interval-ms 0 \
    --mentions "@agent" > /tmp/gateway.log 2>&1 &

GATEWAY_PID=$!

# Wait for gateway to start
sleep 3

# Check if gateway is running
if ! kill -0 $GATEWAY_PID 2>/dev/null; then
    echo -e "${RED}✗ Gateway failed to start${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Gateway running (PID ${GATEWAY_PID})${NC}"
echo ""

# Start the Gateway Monitor
echo -e "${BLUE}Starting Gateway Monitor...${NC}"
"$SCRIPT_DIR/../target/debug/ccos-gateway-monitor" \
    --gateway-url ${GATEWAY_URL} \
    --token "${DEMO_ADMIN_TOKEN}" &
MONITOR_PID=$!

# Wait for monitor to start
sleep 2

# Check if monitor is running
if ! kill -0 $MONITOR_PID 2>/dev/null; then
    echo -e "${RED}✗ Monitor failed to start${NC}"
    exit 1
fi

# echo -e "${GREEN}✓ Monitor running (PID ${MONITOR_PID})${NC}"
# echo ""

# echo -e "${GREEN}==============================${NC}"
# echo -e "${GREEN}✓ Demo is running!${NC}"
# echo ""
# echo -e "${YELLOW}Next steps:${NC}"
# echo "  1. In a NEW terminal, start ccos-chat:"
# echo -e "     ${BLUE}cd $(pwd) && cargo run --bin ccos-chat${NC}"
# echo ""
# echo "  2. Send a message in ccos-chat to trigger an agent"
# echo ""
# echo "  3. Watch the monitor window for real-time updates!"
# echo ""
# echo -e "${YELLOW}Press Ctrl+C in this terminal to stop${NC}"
# echo ""

# Keep the script running
wait
