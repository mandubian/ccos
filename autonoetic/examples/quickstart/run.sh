#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

WORKDIR="${1:-/tmp/autonoetic-quickstart}"
AGENT_ID="${2:-demo_$(date +%s)}"
MODE="${3:-openrouter_gfl}"
CONFIG_PATH="${WORKDIR}/config.yaml"
AGENTS_DIR="${WORKDIR}/agents"
AGENT_DIR="${AGENTS_DIR}/${AGENT_ID}"

mkdir -p "${WORKDIR}"

cat > "${CONFIG_PATH}" <<EOF
agents_dir: "${AGENTS_DIR}"
port: 4000
ofp_port: 4200
tls: false
EOF

cd "${PROJECT_ROOT}"

if [[ -d "${AGENT_DIR}" && "${AUTONOETIC_QUICKSTART_RESET:-0}" == "1" ]]; then
  echo "==> Resetting existing agent '${AGENT_ID}'"
  rm -rf "${AGENT_DIR}"
fi

if [[ -d "${AGENT_DIR}" ]]; then
  echo "==> Reusing existing agent '${AGENT_ID}' at ${AGENT_DIR}"
else
  echo "==> Initializing agent '${AGENT_ID}'"
  cargo run -p autonoetic -- --config "${CONFIG_PATH}" agent init "${AGENT_ID}" --template coder
fi

SKILL_PATH="${AGENT_DIR}/SKILL.md"

if [[ "${MODE}" == "openrouter_gfl" ]]; then
  if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    echo "ERROR: OPENROUTER_API_KEY is required for mode=openrouter_gfl" >&2
    echo "Set it and re-run, or use mode=smoke for local interactive exit test." >&2
    exit 1
  fi
  sed -i 's/provider: "openai"/provider: "openrouter"/' "${SKILL_PATH}"
  sed -i 's/model: "gpt-4o"/model: "google\/gemini-3-flash-preview"/' "${SKILL_PATH}"
  echo "==> Running headless model call via OpenRouter (google/gemini-3-flash-preview)"
  cargo run -p autonoetic -- --config "${CONFIG_PATH}" agent run "${AGENT_ID}" "Reply with one short sentence to confirm the runtime is working." --headless
elif [[ "${MODE}" == "smoke" ]]; then
  sed -i 's/provider: "openai"/provider: "ollama"/' "${SKILL_PATH}"
  echo "==> Running interactive mode smoke test (/exit)"
  printf '/exit\n' | cargo run -p autonoetic -- --config "${CONFIG_PATH}" agent run "${AGENT_ID}" --interactive
else
  echo "ERROR: unsupported mode '${MODE}' (expected: openrouter_gfl|smoke)" >&2
  exit 1
fi

echo
echo "Quickstart complete."
echo "Config: ${CONFIG_PATH}"
echo "Agent dir: ${AGENTS_DIR}/${AGENT_ID}"
echo
echo "Run again with smoke mode:"
echo "  bash examples/quickstart/run.sh \"${WORKDIR}\" \"${AGENT_ID}\" smoke"
