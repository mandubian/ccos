#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

# Local dev federation identity for gateway startup.
export AUTONOETIC_NODE_ID="dev-node-1"
export AUTONOETIC_NODE_NAME="autonoetic-dev-node"
export AUTONOETIC_SHARED_SECRET="dev-shared-secret-change-me"

if [[ $# -eq 0 ]]; then
  echo "Starting gateway with AUTONOETIC_* environment variables..."
  exec cargo run -p autonoetic -- gateway start
fi

# Optional: run any custom command with the same AUTONOETIC_* env.
exec "$@"
