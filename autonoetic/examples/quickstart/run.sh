#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

WORKDIR="${1:-/tmp/autonoetic-quickstart}"
AGENT_ID="${2:-demo_$(date +%s)}"
CONFIG_PATH="${WORKDIR}/config.yaml"
AGENTS_DIR="${WORKDIR}/agents"

mkdir -p "${WORKDIR}"

cat > "${CONFIG_PATH}" <<EOF
agents_dir: "${AGENTS_DIR}"
port: 4000
ofp_port: 4200
tls: false
EOF

cd "${PROJECT_ROOT}"

echo "==> Initializing agent '${AGENT_ID}'"
cargo run -p autonoetic -- --config "${CONFIG_PATH}" agent init "${AGENT_ID}" --template coder

SKILL_PATH="${AGENTS_DIR}/${AGENT_ID}/SKILL.md"
sed -i 's/provider: "openai"/provider: "ollama"/' "${SKILL_PATH}"

echo "==> Running interactive mode smoke test (/exit)"
printf '/exit\n' | cargo run -p autonoetic -- --config "${CONFIG_PATH}" agent run "${AGENT_ID}" --interactive

echo
echo "Quickstart complete."
echo "Config: ${CONFIG_PATH}"
echo "Agent dir: ${AGENTS_DIR}/${AGENT_ID}"
echo
echo "Optional next step (requires local Ollama with a model):"
echo "  cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" agent run \"${AGENT_ID}\" \"Say hello\" --headless"
