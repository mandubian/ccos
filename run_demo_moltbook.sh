#!/bin/bash
if [ -z "${BASH_VERSION:-}" ]; then
    exec bash "$0" "$@"
fi
set -e

# run_demo_moltbook.sh - Demo script for Gateway + Agent + Moltbook skill onboarding
#
# This script demonstrates the full CCOS Gateway-Agent architecture with:
# - Mock Moltbook server (simulates external API)
# - Chat Gateway (Sheriff - manages sessions & capabilities)
# - Agent (Deputy - processes messages, executes capabilities)
#
# Usage:
#   ./run_demo_moltbook.sh
#
# The demo will:
# 1. Start all services
# 2. Trigger a webhook to create a session
# 3. Show the agent connecting and receiving messages
# 4. Demonstrate capability execution through the Gateway

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=========================================${NC}"
echo -e "${BLUE}CCOS Gateway-Agent Moltbook Demo${NC}"
echo -e "${BLUE}=========================================${NC}"
echo ""

# Cleanup function
cleanup() {
    echo ""
    echo -e "${YELLOW}Cleaning up...${NC}"
    # Kill by binary name
    pkill -f ccos-chat-gateway 2>/dev/null || true
    pkill -f ccos-agent 2>/dev/null || true
    pkill -f mock-moltbook 2>/dev/null || true
    # Kill by port
    for port in 8765 8822 8833; do
        fuser -k -n tcp $port 2>/dev/null || true
    done
    # Kill all background jobs
    kill $MOLTBOOK_PID $GATEWAY_PID 2>/dev/null || true
    pkill -f ccos-agent 2>/dev/null || true
    echo -e "${GREEN}✓ Cleanup complete${NC}"
}
trap cleanup INT TERM EXIT

# Set environment
export RUST_LOG="info,ccos_agent=debug"
export CCOS_QUARANTINE_KEY="YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE="

# Build binaries
echo -e "${BLUE}[1/5] Building binaries...${NC}"
cargo build --bin ccos-chat-gateway --bin ccos-agent --bin mock-moltbook 2>&1 | grep -E "(Compiling|Finished|error)" || true
echo -e "${GREEN}✓ Binaries built${NC}"
echo ""

# Start Mock Moltbook
echo -e "${BLUE}[2/5] Starting Mock Moltbook server...${NC}"
# Check if port 8765 is in use
if lsof -i :8765 > /dev/null 2>&1; then
    echo -e "${YELLOW}Port 8765 is in use, attempting to clear...${NC}"
    fuser -k -n tcp 8765 2>/dev/null || true
    sleep 1
fi
./target/debug/mock-moltbook > /tmp/moltbook.log 2>&1 &
MOLTBOOK_PID=$!
echo -e "${GREEN}✓ Mock Moltbook started (PID: $MOLTBOOK_PID)${NC}"
echo -e "  ${YELLOW}Waiting for server to be ready...${NC}"

# Wait for Moltbook to be ready
for i in {1..10}; do
    if curl -s http://localhost:8765/ > /dev/null 2>&1; then
        echo -e "${GREEN}✓ Mock Moltbook is ready${NC}"
        break
    fi
    sleep 1
    if [ $i -eq 10 ]; then
        echo -e "${RED}✗ Mock Moltbook failed to start${NC}"
        exit 1
    fi
done
echo ""

# Start Chat Gateway
echo -e "${BLUE}[3/5] Starting Chat Gateway...${NC}"
# Check if ports are in use
for port in 8822 8833; do
    if lsof -i :$port > /dev/null 2>&1; then
        echo -e "${YELLOW}Port $port is in use, attempting to clear...${NC}"
        fuser -k -n tcp $port 2>/dev/null || true
        sleep 1
    fi
done

export CCOS_GATEWAY_SPAWN_AGENTS=1
export CCOS_AGENT_BINARY="$(pwd)/target/debug/ccos-agent"

# Agent configuration
export CCOS_AGENT_CONFIG_PATH="$(pwd)/config/agent_config.toml"
export CCOS_AGENT_ENABLE_LLM=true
# export CCOS_LLM_PROFILE=${CCOS_LLM_PROFILE:-"google:gemini-1.5-pro"}

# Pass API keys through (will be picked up by agent config logic)
export GEMINI_API_KEY=${GEMINI_API_KEY}
export OPENROUTER_API_KEY=${OPENROUTER_API_KEY}

# Skill URL hints: let the agent resolve "moltbook" -> local mock server
export CCOS_SKILL_URL_HINTS="moltbook=http://localhost:8765/skill.md"

./target/debug/ccos-chat-gateway serve \
    --bind-addr 127.0.0.1:8822 \
    --connector-bind-addr 127.0.0.1:8833 \
    --connector-secret "demo-secret-key" \
    --min-send-interval-ms 0 \
    --allow-senders "user1" \
    --allow-channels "moltbook-demo" \
    --mentions "@agent" \
    --keywords "onboard,done,ok,yes,no,user,agent,post" \
    > /tmp/gateway.log 2>&1 &
GATEWAY_PID=$!
echo -e "${GREEN}✓ Gateway started (PID: $GATEWAY_PID)${NC}"
echo -e "  ${YELLOW}Waiting for Gateway to be ready...${NC}"

# Wait for Gateway to be ready
for i in {1..10}; do
    if curl -s http://127.0.0.1:8822/chat/health > /dev/null 2>&1; then
        echo -e "${GREEN}✓ Gateway is ready${NC}"
        break
    fi
    sleep 1
    if [ $i -eq 10 ]; then
        echo -e "${RED}✗ Gateway failed to start${NC}"
        exit 1
    fi
done
echo ""

# Instruct user to start chat
echo -e "${BLUE}[4/5] Services ready! Please start the interactive chat to begin...${NC}"
echo -e "  ${YELLOW}Run this in a new terminal:${NC}"
echo -e "  ${CYAN}./target/debug/ccos-chat --user-id user1 --channel-id moltbook-demo --status-url http://localhost:8765${NC}"
echo -e "  ${YELLOW}Then type in the chat:${NC}"
echo -e "  ${WHITE}@agent onboard moltbook${NC}"
echo ""

# Wait for session creation and agent spawn
echo -e "${BLUE}[5/5] Waiting for session and agent...${NC}"
echo -e "  ${YELLOW}Monitoring Gateway logs...${NC}"
echo ""

# Show live logs until user stops with Ctrl+C
echo -e "${BLUE}=========================================${NC}"
echo -e "${BLUE}Live Logs - Watching full onboarding${NC}"
echo -e "${BLUE}=========================================${NC}"
echo ""
echo "The agent should:"
echo "  1. Load the Moltbook skill"
echo "  2. Register agent with Moltbook"
echo "  3. Request human verification"
echo "  4. Setup heartbeat"
echo "  5. Report skill is operational"
echo ""
echo "Press Ctrl+C to stop viewing logs and see summary"
echo ""
tail -f /tmp/gateway.log /tmp/moltbook.log 2>/dev/null || true

echo ""
echo -e "${BLUE}=========================================${NC}"
echo -e "${GREEN}Logs stopped${NC}"
echo -e "${BLUE}=========================================${NC}"
echo ""
echo "Summary:"
echo "  - Mock Moltbook: http://localhost:8765 (PID: $MOLTBOOK_PID)"
echo "  - Gateway: http://localhost:8822 (PID: $GATEWAY_PID)"
echo ""
echo "Logs:"
echo "  - Gateway: /tmp/gateway.log"
echo "  - Moltbook: /tmp/moltbook.log"
echo ""
echo "To test manually:"
echo "  1. Check Gateway health: curl http://localhost:8822/chat/health"
echo "  2. Send message: curl -X POST http://localhost:8833/connector/loopback/inbound ..."
echo "  3. View Moltbook status: curl http://localhost:8765/status"
echo ""
echo "Press Ctrl+C to stop all services"
echo ""

# Keep script running
wait
