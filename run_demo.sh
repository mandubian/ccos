#!/bin/bash
set -e

# Cleanup function
cleanup() {
    echo "Stopping background processes..."
    # Aggressively kill by name pattern to catch zombies
    pkill -f ccos-chat-gateway || true
    pkill -f ccos-agent || true
    pkill -f mock-moltbook || true
    kill $(jobs -p) 2>/dev/null
}
trap cleanup EXIT
export RUST_LOG="info"

# Enable "Smart Mode"
# 1. Set your key: export OPENROUTER_API_KEY=sk-or-... 
# 2. Point to config: 
export CCOS_AGENT_CONFIG_PATH=$(pwd)/config/agent_config.toml

# 3. (Optional) Force specific provider/key:
#    export CCOS_LLM_PROVIDER=openai
#    export CCOS_LLM_API_KEY=sk-...

# 1. Build Binaries
echo "🏗️  Building binaries..."
cargo build --bin ccos-chat-gateway --bin ccos-agent --bin mock-moltbook

# 2. Start Mock Moltbook (Port 8765)
echo "📚 Starting Mock Moltbook on port 8765..."
./target/debug/mock-moltbook > mock_moltbook.log 2>&1 &
MOLTBOOK_PID=$!
sleep 1

# 3. Start Chat Gateway (Port 8822, Connector 8833)
# We configure it to:
# - Spawn agents automatically (CCOS_GATEWAY_SPAWN_AGENTS=1)
# - Use our compiled agent binary (CCOS_AGENT_BINARY)
# - Allow messages from 'user1' in 'channel1'
# - Require '@agent' mention to trigger
echo "🛡️  Starting Chat Gateway on port 8822..."
export CCOS_GATEWAY_SPAWN_AGENTS=1
export CCOS_AGENT_BINARY="./target/debug/ccos-agent"
export CCOS_QUARANTINE_KEY="YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=" # 32 bytes base64 (44 chars)

./target/debug/ccos-chat-gateway serve \
    --bind-addr 127.0.0.1:8822 \
    --connector-bind-addr 127.0.0.1:8833 \
    --connector-secret "super-secret" \
    --allow-senders "user1" \
    --allow-channels "channel1" \
    --mentions "@agent" \
    --outbound-url "http://127.0.0.1:8765/api/post-to-feed" \
    --min-send-interval-ms 0 \
    > gateway.log 2>&1 &
GATEWAY_PID=$!

echo "⏳ Waiting for services to startup..."
sleep 5

# 4. Trigger Inbound Message
echo "📨 Sending Trigger Message to Connector (Port 8833)..."
curl -v -X POST http://127.0.0.1:8833/connector/loopback/inbound \
    -H "Content-Type: application/json" \
    -H "x-ccos-connector-secret: super-secret" \
    -d '{
        "channel_id": "channel1",
        "sender_id": "user1",
        "text": "Hello @agent, please help me with a task!",
        "timestamp": "'$(date -u +"%Y-%m-%dT%H:%M:%SZ")'"
    }'

echo ""
echo "✅ Message sent. Tailing logs to show agent spawning..."
echo "-----------------------------------------------------"

# Tail logs for a few seconds to show activity
timeout 10s tail -f gateway.log

echo "-----------------------------------------------------"
echo "Check gateway.log and mock_moltbook.log for full details."
