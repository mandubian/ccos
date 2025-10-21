#!/usr/bin/env bash
set -euo pipefail

# Export chats to a new repository folder, compressing JSONs and copying MDs.
# This script wraps scripts/migrate_chats_to_repo.py and optionally inits a git repo
# and pushes to a remote.

usage() {
  cat <<'USAGE'
Usage: export_chats_repo.sh [options]

Options:
  --output-dir DIR     Output directory for the new repo (default ../ccos-chats)
  --remote-url URL     Optional remote git URL to add as origin
  --push               If set, push the initial commit to the remote (requires --remote-url)
  --fetch-lfs          If set, script will attempt to `git lfs pull --include="chats/*"` before export
  --use-gzip           Prefer gzip instead of zstd
  --dry-run            Show actions without making changes
  --help               Show this help

Example:
  ./scripts/export_chats_repo.sh --output-dir ../ccos-chats --remote-url git@github.com:you/ccos-chats.git --push --fetch-lfs

This script will NOT remove files from the main repo. Run the history-clean script later when you're ready.
USAGE
}

OUTPUT_DIR=../ccos-chats
REMOTE_URL=""
PUSH="no"
FETCH_LFS="no"
USE_GZIP="no"
DRY_RUN="no"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir) OUTPUT_DIR="$2"; shift 2;;
    --remote-url) REMOTE_URL="$2"; shift 2;;
    --push) PUSH="yes"; shift;;
    --fetch-lfs) FETCH_LFS="yes"; shift;;
    --use-gzip) USE_GZIP="yes"; shift;;
    --dry-run) DRY_RUN="yes"; shift;;
    --help) usage; exit 0;;
    *) echo "Unknown arg: $1"; usage; exit 1;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Repo root: $REPO_ROOT"
echo "Output dir: $OUTPUT_DIR"

PY_SCRIPT="$REPO_ROOT/scripts/migrate_chats_to_repo.py"
if [ ! -f "$PY_SCRIPT" ]; then
  echo "Missing migration python script: $PY_SCRIPT" >&2
  exit 1
fi

CMD=(python3 "$PY_SCRIPT" --output-dir "${OUTPUT_DIR}")
if [ "$FETCH_LFS" = "yes" ]; then
  CMD+=(--fetch-lfs)
fi
if [ "$USE_GZIP" = "yes" ]; then
  CMD+=(--use-gzip)
fi

if [ "$DRY_RUN" = "yes" ]; then
  CMD+=(--dry-run)
fi

echo "Running export command: ${CMD[*]}"
if [ "$DRY_RUN" = "yes" ]; then
  "${CMD[@]}"
  echo "Dry-run complete. No files written."
  exit 0
fi

"${CMD[@]}"

echo "Export finished. Output is in: ${OUTPUT_DIR}"

# Init git repo in output dir if not present
if [ ! -d "${OUTPUT_DIR}/.git" ]; then
  echo "Initializing git repo in ${OUTPUT_DIR}"
  git -C "${OUTPUT_DIR}" init
  git -C "${OUTPUT_DIR}" add .
  git -C "${OUTPUT_DIR}" commit -m "Import compressed chats from main repo"
fi

if [ -n "$REMOTE_URL" ]; then
  echo "Adding remote $REMOTE_URL"
  git -C "${OUTPUT_DIR}" remote add origin "$REMOTE_URL" || true
  if [ "$PUSH" = "yes" ]; then
    echo "Pushing initial commit to $REMOTE_URL"
    git -C "${OUTPUT_DIR}" push -u origin main || git -C "${OUTPUT_DIR}" push -u origin master || true
    echo "Push attempted. Verify remote repository now."
  else
    echo "Remote added but not pushed (use --push to push)"
  fi
fi

echo "Export script done. Review ${OUTPUT_DIR} before doing any destructive changes to the original repo."
