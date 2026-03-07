#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

WORKDIR="${1:-/tmp/autonoetic-quickstart}"
AGENT_ID="${2:-field_journal}"
MODE="${3:-openrouter_gfl}"
CONFIG_PATH="${WORKDIR}/config.yaml"
AGENTS_DIR="${WORKDIR}/agents"
AGENT_DIR="${AGENTS_DIR}/${AGENT_ID}"
SKILL_PATH="${AGENT_DIR}/SKILL.md"
RUNTIME_LOCK_PATH="${AGENT_DIR}/runtime.lock"
SESSION_ID="quickstart-session-${AGENT_ID}"
FOLLOWUP_SESSION_ID="${SESSION_ID}-new"
CHANNEL_ID="terminal:quickstart:${AGENT_ID}"

wait_for_port() {
  local host="$1"
  local port="$2"
  local retries="${3:-50}"
  for ((i=0; i<retries; i++)); do
    if (echo >"/dev/tcp/${host}/${port}") >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.2
  done
  echo "ERROR: timed out waiting for ${host}:${port}" >&2
  return 1
}

cleanup() {
  if [[ -n "${GATEWAY_PID:-}" ]] && kill -0 "${GATEWAY_PID}" >/dev/null 2>&1; then
    kill "${GATEWAY_PID}" >/dev/null 2>&1 || true
    wait "${GATEWAY_PID}" >/dev/null 2>&1 || true
  fi
}

run_chat() {
  local session_id="$1"
  local sender_id="$2"
  cargo run -p autonoetic -- --config "${CONFIG_PATH}" chat "${AGENT_ID}" \
    --sender-id "${sender_id}" \
    --channel-id "${CHANNEL_ID}" \
    --session-id "${session_id}"
}

trap cleanup EXIT

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
  echo "==> Reusing existing sample agent '${AGENT_ID}' at ${AGENT_DIR}"
else
  echo "==> Installing sample agent '${AGENT_ID}'"
  mkdir -p "${AGENT_DIR}/state" "${AGENT_DIR}/history" "${AGENT_DIR}/skills" "${AGENT_DIR}/scripts"
  cp "${SCRIPT_DIR}/sample_agent/SKILL.md" "${SKILL_PATH}"
  cp "${SCRIPT_DIR}/sample_agent/runtime.lock" "${RUNTIME_LOCK_PATH}"
  sed -i "s/__AGENT_ID__/${AGENT_ID}/g" "${SKILL_PATH}"
fi

if [[ "${MODE}" == "openrouter_gfl" || "${MODE}" == "openrouter_gfl_manual" ]]; then
  if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    echo "ERROR: OPENROUTER_API_KEY is required for mode=${MODE}" >&2
    echo "Set it and re-run, or use mode=smoke for a gateway/chat startup check." >&2
    exit 1
  fi
  sed -i 's/provider: ".*"/provider: "openrouter"/' "${SKILL_PATH}"
  sed -i 's/model: ".*"/model: "google\/gemini-3-flash-preview"/' "${SKILL_PATH}"
  export AUTONOETIC_NODE_ID="${AUTONOETIC_NODE_ID:-quickstart-node}"
  export AUTONOETIC_NODE_NAME="${AUTONOETIC_NODE_NAME:-Quickstart Gateway}"
  export AUTONOETIC_SHARED_SECRET="${AUTONOETIC_SHARED_SECRET:-quickstart-secret}"
  echo "==> Starting gateway"
  cargo run -p autonoetic -- --config "${CONFIG_PATH}" gateway start >/dev/null 2>&1 &
  GATEWAY_PID=$!
  wait_for_port 127.0.0.1 4000
  if [[ "${MODE}" == "openrouter_gfl" ]]; then
    echo "==> Running scripted terminal chat quickstart"
    echo "    Pass 1: same-session continuity via session context"
    printf 'Remember that my project codename is Atlas.\nWhat did I just ask you to remember?\n/exit\n' \
      | run_chat "${SESSION_ID}" "quickstart"
    echo "==> Running second scripted pass"
    echo "    Pass 2: fresh-session recall via durable memory"
    printf 'What is my project codename?\n/exit\n' \
      | run_chat "${FOLLOWUP_SESSION_ID}" "quickstart"
  else
    echo "==> Starting interactive terminal chat"
    echo "    Try:"
    echo "      Remember that my project codename is Atlas."
    echo "      What did I just ask you to remember?"
    echo "    Then exit and restart chat with a new session id:"
    echo "      cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" chat \"${AGENT_ID}\" --sender-id quickstart --channel-id \"${CHANNEL_ID}\" --session-id \"${FOLLOWUP_SESSION_ID}\""
    echo "      What is my project codename?"
    run_chat "${SESSION_ID}" "quickstart"
  fi
elif [[ "${MODE}" == "smoke" ]]; then
  sed -i 's/provider: ".*"/provider: "ollama"/' "${SKILL_PATH}"
  sed -i 's/model: ".*"/model: "llama3.2"/' "${SKILL_PATH}"
  export AUTONOETIC_NODE_ID="${AUTONOETIC_NODE_ID:-quickstart-node}"
  export AUTONOETIC_NODE_NAME="${AUTONOETIC_NODE_NAME:-Quickstart Gateway}"
  export AUTONOETIC_SHARED_SECRET="${AUTONOETIC_SHARED_SECRET:-quickstart-secret}"
  echo "==> Starting gateway"
  cargo run -p autonoetic -- --config "${CONFIG_PATH}" gateway start >/dev/null 2>&1 &
  GATEWAY_PID=$!
  wait_for_port 127.0.0.1 4000
  echo "==> Running terminal chat smoke test (/exit)"
  printf '/exit\n' \
    | cargo run -p autonoetic -- --config "${CONFIG_PATH}" chat "${AGENT_ID}"
else
  echo "ERROR: unsupported mode '${MODE}' (expected: openrouter_gfl|openrouter_gfl_manual|smoke)" >&2
  exit 1
fi

echo
echo "Quickstart complete."
echo "Config: ${CONFIG_PATH}"
echo "Agent dir: ${AGENTS_DIR}/${AGENT_ID}"
echo "Session: ${SESSION_ID}"
echo "Fresh session for durable-memory check: ${FOLLOWUP_SESSION_ID}"
echo
echo "Inspect memory:"
echo "  cat ${AGENT_DIR}/state/latest_fact.txt"
echo "  cat ${AGENT_DIR}/state/latest_fact_label.txt"
echo "  ls ${AGENT_DIR}/state/sessions"
echo
echo "Inspect traces:"
echo "  cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" trace sessions --agent \"${AGENT_ID}\""
echo "  cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" trace show \"${SESSION_ID}\" --agent \"${AGENT_ID}\""
echo "  cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" trace show \"${FOLLOWUP_SESSION_ID}\" --agent \"${AGENT_ID}\""
echo
echo "Run again with smoke mode:"
echo "  bash examples/quickstart/run.sh \"${WORKDIR}\" \"${AGENT_ID}\" smoke"
