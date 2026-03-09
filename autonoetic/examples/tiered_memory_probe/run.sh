#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

WORKDIR="${1:-/tmp/autonoetic-tiered-memory-probe}"
AGENT_ID="${2:-builder_memory_probe}"
CONFIG_PATH="${WORKDIR}/config.yaml"
AGENTS_DIR="${WORKDIR}/agents"
AGENT_DIR="${AGENTS_DIR}/${AGENT_ID}"
SKILL_PATH="${AGENT_DIR}/SKILL.md"
RUNTIME_LOCK_PATH="${AGENT_DIR}/runtime.lock"
SESSION_ID="tiered-memory-probe-session-${AGENT_ID}"
CHANNEL_ID="terminal:tiered-memory-probe:${AGENT_ID}"

PROMPT="Build a recurring market-observer worker that wakes every 20 seconds. It must keep only the minimal operational checkpoint needed for the next tick, but also accumulate reusable findings that future analyst workers can query by topic and timeframe without replaying raw history files. Assume those future analyst workers do not have direct filesystem access to this worker directory."

capture_child_agents() {
  if [[ ! -d "${AGENTS_DIR}" ]]; then
    return 0
  fi
  find "${AGENTS_DIR}" -mindepth 1 -maxdepth 1 -type d \
    ! -name '.gateway' \
    ! -name "${AGENT_ID}" \
    -printf '%f\n' | sort
}

find_new_child_agent() {
  local before="$1"
  local child
  while IFS= read -r child; do
    [[ -z "${child}" ]] && continue
    if ! grep -Fqx "${child}" <<< "${before}"; then
      printf '%s\n' "${child}"
      return 0
    fi
  done < <(capture_child_agents)
  return 1
}

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
  cargo run -p autonoetic -- --config "${CONFIG_PATH}" chat "${AGENT_ID}" \
    --sender-id tiered-memory-probe \
    --channel-id "${CHANNEL_ID}" \
    --session-id "${session_id}"
}

trap cleanup EXIT

mkdir -p "${WORKDIR}"

cat > "${CONFIG_PATH}" <<EOF
agents_dir: "${AGENTS_DIR}"
port: 4010
ofp_port: 4210
tls: false
background_scheduler_enabled: true
background_tick_secs: 1
background_min_interval_secs: 1
max_background_due_per_tick: 8
EOF

cd "${PROJECT_ROOT}"

if [[ -d "${AGENT_DIR}" && "${AUTONOETIC_TIERED_MEMORY_PROBE_RESET:-1}" == "1" ]]; then
  echo "==> Resetting existing probe workspace"
  rm -rf "${AGENT_DIR}"
  if [[ -d "${AGENTS_DIR}" ]]; then
    find "${AGENTS_DIR}" -mindepth 1 -maxdepth 1 -type d ! -name '.gateway' -exec rm -rf {} +
  fi
fi

if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
  echo "ERROR: OPENROUTER_API_KEY is required" >&2
  exit 1
fi

if [[ ! -d "${AGENT_DIR}" ]]; then
  echo "==> Installing probe builder agent '${AGENT_ID}'"
  mkdir -p "${AGENT_DIR}/state" "${AGENT_DIR}/history" "${AGENT_DIR}/skills" "${AGENT_DIR}/scripts"
  cp "${SCRIPT_DIR}/sample_agent/SKILL.md" "${SKILL_PATH}"
  cp "${SCRIPT_DIR}/sample_agent/runtime.lock" "${RUNTIME_LOCK_PATH}"
  sed -i "s/name: \"builder_memory_probe\"/name: \"${AGENT_ID}\"/" "${SKILL_PATH}"
  sed -i "s/id: \"builder_memory_probe\"/id: \"${AGENT_ID}\"/" "${SKILL_PATH}"
fi

sed -i 's/provider: ".*"/provider: "openrouter"/' "${SKILL_PATH}"
sed -i 's/model: ".*"/model: "google\/gemini-3-flash-preview"/' "${SKILL_PATH}"

export AUTONOETIC_NODE_ID="${AUTONOETIC_NODE_ID:-tiered-memory-probe-node}"
export AUTONOETIC_NODE_NAME="${AUTONOETIC_NODE_NAME:-Tiered Memory Probe Gateway}"
export AUTONOETIC_SHARED_SECRET="${AUTONOETIC_SHARED_SECRET:-tiered-memory-probe-secret}"

echo "==> Starting gateway"
cargo run -p autonoetic -- --config "${CONFIG_PATH}" gateway start >/dev/null 2>&1 &
GATEWAY_PID=$!
wait_for_port 127.0.0.1 4010

BEFORE_CHILDREN="$(capture_child_agents)"

echo "==> Sending probe request"
printf '%s\n/exit\n' "${PROMPT}" | run_chat "${SESSION_ID}"

CHILD_AGENT_ID=""
for _ in $(seq 1 30); do
  if CHILD_AGENT_ID="$(find_new_child_agent "${BEFORE_CHILDREN}")"; then
    break
  fi
  sleep 0.2
done

if [[ -z "${CHILD_AGENT_ID}" ]]; then
  echo "PROBE_RESULT: FAIL"
  echo "reason: no child agent installed"
  exit 2
fi

CHILD_DIR="${AGENTS_DIR}/${CHILD_AGENT_ID}"
CHILD_SKILL="${CHILD_DIR}/SKILL.md"
CHILD_HISTORY="${CHILD_DIR}/history/causal_chain.jsonl"

echo "==> Installed child agent: ${CHILD_AGENT_ID}"

# Give scheduler time for first tick.
for _ in $(seq 1 25); do
  if [[ -f "${CHILD_HISTORY}" ]]; then
    break
  fi
  sleep 0.2
done

HAS_TIER1_STATE=0
if [[ -d "${CHILD_DIR}/state" ]]; then
  if find "${CHILD_DIR}/state" -mindepth 1 -maxdepth 1 -type f ! -name 'reevaluation.json' | grep -q .; then
    HAS_TIER1_STATE=1
  fi
fi

HAS_OUTPUT_MEMORY_KEYS=1
if [[ -f "${CHILD_SKILL}" ]]; then
  if rg -n "memory_keys:\s*\[\s*\]" "${CHILD_SKILL}" >/dev/null 2>&1; then
    HAS_OUTPUT_MEMORY_KEYS=0
  fi
else
  HAS_OUTPUT_MEMORY_KEYS=0
fi

HAS_SDK_MARKERS=0
if rg -n "autonoetic_sdk|memory\.remember|memory\.recall|memory\.search" "${CHILD_DIR}" -g '!history/**' >/dev/null 2>&1; then
  HAS_SDK_MARKERS=1
fi

HAS_MEMORY_EVENTS=0
if [[ -f "${CHILD_HISTORY}" ]]; then
  if rg -n '"category":"memory"' "${CHILD_HISTORY}" >/dev/null 2>&1; then
    HAS_MEMORY_EVENTS=1
  fi
fi

echo
echo "==> Probe diagnostics"
echo "child_agent_id=${CHILD_AGENT_ID}"
echo "tier1_state_present=${HAS_TIER1_STATE}"
echo "output_contract_memory_keys_non_empty=${HAS_OUTPUT_MEMORY_KEYS}"
echo "sdk_or_memory_markers_in_code=${HAS_SDK_MARKERS}"
echo "memory_events_in_causal_trace=${HAS_MEMORY_EVENTS}"

if [[ ${HAS_TIER1_STATE} -eq 1 && ${HAS_OUTPUT_MEMORY_KEYS} -eq 1 && ${HAS_SDK_MARKERS} -eq 1 && ${HAS_MEMORY_EVENTS} -eq 1 ]]; then
  echo "PROBE_RESULT: PASS"
  exit 0
fi

echo "PROBE_RESULT: FAIL"
echo "hint: worker likely stayed file-only or did not publish reusable memory"
exit 3
