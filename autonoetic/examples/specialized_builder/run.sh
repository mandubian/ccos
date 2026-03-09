#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

WORKDIR="${1:-/tmp/autonoetic-specialized-builder}"
AGENT_ID="${2:-builder_agent}"
MODE="${3:-demo_fibonacci}"
CONFIG_PATH="${WORKDIR}/config.yaml"
AGENTS_DIR="${WORKDIR}/agents"
AGENT_DIR="${AGENTS_DIR}/${AGENT_ID}"
SKILL_PATH="${AGENT_DIR}/SKILL.md"
RUNTIME_LOCK_PATH="${AGENT_DIR}/runtime.lock"
SESSION_ID="specialized-builder-session-${AGENT_ID}"
CHANNEL_ID="terminal:specialized-builder:${AGENT_ID}"

EXPECTED_INTERVAL_SECS="${AUTONOETIC_EXPECTED_INTERVAL_SECS:-20}"
INTERVAL_TOLERANCE_SECS="${AUTONOETIC_INTERVAL_TOLERANCE_SECS:-6}"
CADENCE_WAIT_TIMEOUT_SECS="${AUTONOETIC_CADENCE_WAIT_TIMEOUT_SECS:-90}"
REQUIRED_SCHEDULER_TICKS="${AUTONOETIC_REQUIRED_SCHEDULER_TICKS:-2}"

# Automatically scale timeout if many ticks are requested
MIN_TIMEOUT_SECS=$(( (EXPECTED_INTERVAL_SECS * REQUIRED_SCHEDULER_TICKS) + 30 ))
if (( CADENCE_WAIT_TIMEOUT_SECS < MIN_TIMEOUT_SECS )); then
  echo "==> Scaling CADENCE_WAIT_TIMEOUT_SECS from ${CADENCE_WAIT_TIMEOUT_SECS} to ${MIN_TIMEOUT_SECS} for ${REQUIRED_SCHEDULER_TICKS} ticks"
  CADENCE_WAIT_TIMEOUT_SECS="${MIN_TIMEOUT_SECS}"
fi

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

first_matching_file() {
  local dir="$1"
  local exclude_name="${2:-}"
  if [[ ! -d "${dir}" ]]; then
    return 1
  fi
  find "${dir}" -mindepth 1 -maxdepth 1 -type f \
    $( [[ -n "${exclude_name}" ]] && printf '! -name %q ' "${exclude_name}" ) \
    -print | sort | head -n 1
}

print_dir_files() {
  local dir="$1"
  local heading="$2"
  local exclude_name="${3:-}"
  local found=0
  local file
  if [[ ! -d "${dir}" ]]; then
    echo "==> ${heading}"
    echo "(directory not found: ${dir})"
    return 0
  fi
  echo "==> ${heading}"
  while IFS= read -r file; do
    [[ -z "${file}" ]] && continue
    found=1
    echo "-- ${file}"
    cat "${file}"
    echo
  done < <(find "${dir}" -mindepth 1 -maxdepth 1 -type f $( [[ -n "${exclude_name}" ]] && printf '! -name %q ' "${exclude_name}" ) -print | sort)
  if [[ ${found} -eq 0 ]]; then
    echo "(no files found)"
  fi
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
    --sender-id specialized-builder \
    --channel-id "${CHANNEL_ID}" \
    --session-id "${session_id}"
}

count_background_scheduler_ticks() {
  local child_agent_id="$1"
  local gateway_log="${AGENTS_DIR}/.gateway/history/causal_chain.jsonl"
  if [[ ! -f "${gateway_log}" ]]; then
    echo 0
    return 0
  fi
  rg -c "\"action\":\"background.should_wake.completed\".*\"session_id\":\"background::${child_agent_id}\"|\"session_id\":\"background::${child_agent_id}\".*\"action\":\"background.should_wake.completed\"" "${gateway_log}" || echo 0
}

verify_background_interval() {
  local child_agent_id="$1"
  local expected_secs="$2"
  local tolerance_secs="$3"
  local gateway_log="${AGENTS_DIR}/.gateway/history/causal_chain.jsonl"

  [[ -f "${gateway_log}" ]] || {
    echo "ERROR: gateway causal log not found at ${gateway_log}" >&2
    return 1
  }

  local first_ts second_ts
  first_ts="$(rg "\"action\":\"background.should_wake.completed\".*\"session_id\":\"background::${child_agent_id}\"|\"session_id\":\"background::${child_agent_id}\".*\"action\":\"background.should_wake.completed\"" "${gateway_log}" | sed -n '1s/.*\"timestamp\":\"\([^\"]*\)\".*/\1/p')"
  second_ts="$(rg "\"action\":\"background.should_wake.completed\".*\"session_id\":\"background::${child_agent_id}\"|\"session_id\":\"background::${child_agent_id}\".*\"action\":\"background.should_wake.completed\"" "${gateway_log}" | sed -n '2s/.*\"timestamp\":\"\([^\"]*\)\".*/\1/p')"

  if [[ -z "${first_ts}" || -z "${second_ts}" ]]; then
    echo "ERROR: could not extract two scheduler tick timestamps for ${child_agent_id}" >&2
    return 1
  fi

  local first_epoch second_epoch delta lower upper
  first_epoch="$(date -d "${first_ts}" +%s)"
  second_epoch="$(date -d "${second_ts}" +%s)"
  delta="$((second_epoch - first_epoch))"
  lower="$((expected_secs - tolerance_secs))"
  upper="$((expected_secs + tolerance_secs))"

  echo "==> Scheduler interval check (background.should_wake.completed)"
  echo "first_tick_ts=${first_ts}"
  echo "second_tick_ts=${second_ts}"
  echo "measured_interval_secs=${delta}"
  echo "expected_interval_secs=${expected_secs} +/- ${tolerance_secs}"

  if (( delta < lower || delta > upper )); then
    echo "ERROR: measured interval ${delta}s outside expected range [${lower}, ${upper}]" >&2
    return 1
  fi

  echo "PASS: measured interval ${delta}s is within expected range."
}

print_background_diagnostics() {
  local child_agent_id="$1"
  local gateway_log="${AGENTS_DIR}/.gateway/history/causal_chain.jsonl"

  if [[ ! -f "${gateway_log}" ]]; then
    echo "(gateway log missing at ${gateway_log})" >&2
    return 0
  fi

  echo "==> Recent background events for ${child_agent_id}" >&2
  rg "\"session_id\":\"background::${child_agent_id}\"" "${gateway_log}" | tail -n 30 >&2 || true
}

print_reevaluation_diagnostics() {
  local child_agent_id="$1"
  local reevaluation_path="${AGENTS_DIR}/${child_agent_id}/state/reevaluation.json"

  echo "==> Reevaluation state for ${child_agent_id}" >&2
  if [[ ! -f "${reevaluation_path}" ]]; then
    echo "(missing: ${reevaluation_path})" >&2
    return 0
  fi

  cat "${reevaluation_path}" >&2
  if rg -q '"pending_scheduled_action"\s*:\s*null' "${reevaluation_path}"; then
    echo "NOTE: pending_scheduled_action is null, so wake.skipped(no_executable_work) is expected until work is re-armed." >&2
  fi
}

trap cleanup EXIT

mkdir -p "${WORKDIR}"

cat > "${CONFIG_PATH}" <<EOF
agents_dir: "${AGENTS_DIR}"
port: 4000
ofp_port: 4200
tls: false
background_scheduler_enabled: true
background_tick_secs: 1
background_min_interval_secs: 1
max_background_due_per_tick: 8
EOF

cd "${PROJECT_ROOT}"

if [[ -d "${AGENT_DIR}" && "${AUTONOETIC_SPECIALIZED_BUILDER_RESET:-0}" == "1" ]]; then
  echo "==> Resetting existing builder agent '${AGENT_ID}'"
  rm -rf "${AGENT_DIR}"
  if [[ -d "${AGENTS_DIR}" ]]; then
    find "${AGENTS_DIR}" -mindepth 1 -maxdepth 1 -type d ! -name '.gateway' -exec rm -rf {} +
  fi
fi

if [[ -d "${AGENT_DIR}" ]]; then
  echo "==> Reusing existing builder agent '${AGENT_ID}' at ${AGENT_DIR}"
else
  echo "==> Installing sample builder agent '${AGENT_ID}'"
  mkdir -p "${AGENT_DIR}/state" "${AGENT_DIR}/history" "${AGENT_DIR}/skills" "${AGENT_DIR}/scripts"
  cp "${SCRIPT_DIR}/sample_agent/SKILL.md" "${SKILL_PATH}"
  cp "${SCRIPT_DIR}/sample_agent/runtime.lock" "${RUNTIME_LOCK_PATH}"
  sed -i "s/name: \"sample_agent\"/name: \"${AGENT_ID}\"/" "${SKILL_PATH}"
  sed -i "s/id: \"sample_agent\"/id: \"${AGENT_ID}\"/" "${SKILL_PATH}"
fi

if [[ "${MODE}" == "demo_fibonacci" || "${MODE}" == "manual" ]]; then
  if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    echo "ERROR: OPENROUTER_API_KEY is required for mode=${MODE}" >&2
    echo "Set it and re-run, or use mode=smoke for a startup/exit check." >&2
    exit 1
  fi
  sed -i 's/provider: ".*"/provider: "openrouter"/' "${SKILL_PATH}"
  sed -i 's/model: ".*"/model: "google\/gemini-3-flash-preview"/' "${SKILL_PATH}"
elif [[ "${MODE}" == "smoke" ]]; then
  sed -i 's/provider: ".*"/provider: "ollama"/' "${SKILL_PATH}"
  sed -i 's/model: ".*"/model: "llama3.2"/' "${SKILL_PATH}"
else
  echo "ERROR: unsupported mode '${MODE}' (expected: demo_fibonacci|manual|smoke)" >&2
  exit 1
fi

export AUTONOETIC_NODE_ID="${AUTONOETIC_NODE_ID:-specialized-builder-node}"
export AUTONOETIC_NODE_NAME="${AUTONOETIC_NODE_NAME:-Specialized Builder Gateway}"
export AUTONOETIC_SHARED_SECRET="${AUTONOETIC_SHARED_SECRET:-specialized-builder-secret}"

echo "==> Starting gateway"
cargo run -p autonoetic -- --config "${CONFIG_PATH}" gateway start >/dev/null 2>&1 &
GATEWAY_PID=$!
wait_for_port 127.0.0.1 4000

if [[ "${MODE}" == "demo_fibonacci" ]]; then
  BEFORE_CHILDREN="$(capture_child_agents)"
  echo "==> Sending Fibonacci specialization request"
  printf 'schedule every 20sec next fibonacci series element from previous element computed in last turn\n/exit\n' \
    | run_chat "${SESSION_ID}"

  CHILD_AGENT_ID=""
  for _ in $(seq 1 25); do
    if CHILD_AGENT_ID="$(find_new_child_agent "${BEFORE_CHILDREN}")"; then
      break
    fi
    sleep 0.2
  done

  if [[ -z "${CHILD_AGENT_ID}" ]]; then
    echo "ERROR: no child agent was installed. Inspect builder traces under ${AGENT_DIR}/history" >&2
    exit 1
  fi

  echo "==> Installed child agent: ${CHILD_AGENT_ID}"
  echo "==> Waiting for first worker output"
  for _ in $(seq 1 120); do
    HISTORY_FILE="$(first_matching_file "${AGENTS_DIR}/${CHILD_AGENT_ID}/history" "causal_chain.jsonl" || true)"
    if [[ -n "${HISTORY_FILE}" ]]; then
      break
    fi
    sleep 0.5
  done

  echo "==> Waiting for ${REQUIRED_SCHEDULER_TICKS} scheduler ticks to validate cadence"
  for _ in $(seq 1 "${CADENCE_WAIT_TIMEOUT_SECS}"); do
    ticks="$(count_background_scheduler_ticks "${CHILD_AGENT_ID}")"
    if [[ "${ticks}" -ge "${REQUIRED_SCHEDULER_TICKS}" ]]; then
      break
    fi
    sleep 1
  done

  ticks="$(count_background_scheduler_ticks "${CHILD_AGENT_ID}")"
  if [[ "${ticks}" -lt "${REQUIRED_SCHEDULER_TICKS}" ]]; then
    echo "ERROR: did not observe ${REQUIRED_SCHEDULER_TICKS} background.should_wake.completed events for ${CHILD_AGENT_ID}" >&2
    echo "Observed scheduler ticks: ${ticks}" >&2
    print_background_diagnostics "${CHILD_AGENT_ID}"
    print_reevaluation_diagnostics "${CHILD_AGENT_ID}"
    exit 1
  fi

  verify_background_interval "${CHILD_AGENT_ID}" "${EXPECTED_INTERVAL_SECS}" "${INTERVAL_TOLERANCE_SECS}"

  echo
  print_dir_files "${AGENTS_DIR}/${CHILD_AGENT_ID}/state" "Installed worker state" "reevaluation.json"
  echo
  print_dir_files "${AGENTS_DIR}/${CHILD_AGENT_ID}/history" "Installed worker history" "causal_chain.jsonl"
elif [[ "${MODE}" == "manual" ]]; then
  echo "==> Starting interactive builder chat"
  echo "    Try: schedule every 20sec next fibonacci series element from previous element computed in last turn"
  run_chat "${SESSION_ID}"
else
  echo "==> Running smoke startup check"
  printf '/exit\n' | cargo run -p autonoetic -- --config "${CONFIG_PATH}" chat "${AGENT_ID}"
fi

echo
echo "Specialized builder example complete."
echo "Config: ${CONFIG_PATH}"
echo "Builder agent dir: ${AGENT_DIR}"
echo "Child worker dir: ${AGENTS_DIR}/${CHILD_AGENT_ID:-<installed-child-agent>}"
echo
echo "Inspect traces:"
echo "  cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" trace sessions --agent \"${AGENT_ID}\""
echo "  cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" trace sessions --agent \"${CHILD_AGENT_ID:-<child-agent-id>}\""
echo "  cargo run -p autonoetic -- --config \"${CONFIG_PATH}\" trace show \"background::${CHILD_AGENT_ID:-<child-agent-id>}\" --agent \"${CHILD_AGENT_ID:-<child-agent-id>}\""
