#!/bin/bash
#
# Integration test script for CCOS Agent + Gateway + Mock Moltbook
#
# This script sets up the full loop for testing skill onboarding
#

set -e

echo "========================================="
echo "CCOS Integration Test: Moltbook Onboarding"
echo "========================================="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if binaries exist
check_binary() {
    if [ ! -f "target/debug/$1" ]; then
        echo -e "${RED}Error: $1 binary not found${NC}"
        echo "Run: cargo build --bin $1"
        exit 1
    fi
}

echo "Step 1: Checking binaries..."
check_binary "mock-moltbook"
check_binary "ccos-chat-gateway"
check_binary "ccos-agent"
echo -e "${GREEN}✓ All binaries found${NC}"
echo ""

# Function to cleanup processes on exit
cleanup() {
    echo ""
    echo "Cleaning up..."
    if [ -n "$MOLTBOOK_PID" ]; then
        kill $MOLTBOOK_PID 2>/dev/null || true
        echo "Stopped mock-moltbook (PID: $MOLTBOOK_PID)"
    fi
    if [ -n "$GATEWAY_PID" ]; then
        kill $GATEWAY_PID 2>/dev/null || true
        echo "Stopped ccos-chat-gateway (PID: $GATEWAY_PID)"
    fi
    if [ -n "$AGENT_PID" ]; then
        kill $AGENT_PID 2>/dev/null || true
        echo "Stopped ccos-agent (PID: $AGENT_PID)"
    fi
    exit 0
}

trap cleanup EXIT INT TERM

# Start Mock Moltbook Server
echo "Step 2: Starting Mock Moltbook Server..."
./target/debug/mock-moltbook &
MOLTBOOK_PID=$!
echo "Mock Moltbook started (PID: $MOLTBOOK_PID)"
echo -e "${YELLOW}Waiting for server to start...${NC}"
sleep 2

# Check if server is running
if ! curl -s http://localhost:8765/ > /dev/null; then
    echo -e "${RED}Error: Mock Moltbook failed to start${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Mock Moltbook is running${NC}"
echo ""

# Start CCOS Chat Gateway
echo "Step 3: Starting CCOS Chat Gateway..."
mkdir -p /tmp/ccos_test/approvals /tmp/ccos_test/quarantine
export CCOS_QUARANTINE_KEY="test-key-for-development-only"
export CCOS_LOG_LEVEL=info

./target/debug/ccos-chat-gateway &
GATEWAY_PID=$!
echo "Gateway started (PID: $GATEWAY_PID)"
echo -e "${YELLOW}Waiting for gateway to start...${NC}"
sleep 3

# Check if gateway is running
if ! curl -s http://localhost:8080/chat/health > /dev/null; then
    echo -e "${RED}Error: Gateway failed to start${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Gateway is running${NC}"
echo ""

# Wait for session creation (via webhook simulation)
echo "Step 4: Simulating webhook trigger to create session..."
echo -e "${YELLOW}In a real scenario, a webhook from Moltbook would trigger this${NC}"
echo -e "${YELLOW}For testing, the agent would be started with session details${NC}"
echo ""

# Show test instructions
echo "========================================="
echo "Manual Test Instructions"
echo "========================================="
echo ""
echo "Terminal 1 (Mock Moltbook): Running on PID $MOLTBOOK_PID"
echo "  - View at: http://localhost:8765/"
echo "  - Skill definition: http://localhost:8765/skill.md"
echo ""
echo "Terminal 2 (Gateway): Running on PID $GATEWAY_PID"
echo "  - Health check: http://localhost:8080/chat/health"
echo "  - API docs: See ccos/src/chat/gateway.rs"
echo ""
echo "To test the full loop:"
echo ""
echo "1. Trigger a webhook to the Gateway:"
echo "   curl -X POST http://localhost:8080/webhook/moltbook \\"
echo "     -H 'Content-Type: application/json' \\"
echo "     -d '{\"message\": \"Hello from Moltbook\", \"channel\": \"test\"}'"
echo ""
echo "2. The Gateway will:"
echo "   - Create a new session"
echo "   - Generate an auth token"
echo "   - Spawn the ccos-agent"
echo ""
echo "3. The Agent will:"
echo "   - Connect with the token"
echo "   - Poll /chat/events for messages"
echo "   - Process with LLM (if enabled)"
echo "   - Execute capabilities through Gateway"
echo ""
echo "4. To start agent manually:"
echo "   ./target/debug/ccos-agent \\"
echo "     --token <TOKEN_FROM_GATEWAY_LOGS> \\"
echo "     --session-id <SESSION_ID> \\"
echo "     --gateway-url http://localhost:8080"
echo ""
echo "Press Ctrl+C to stop all services"
echo ""

# Keep script running
wait
