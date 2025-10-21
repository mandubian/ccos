#!/usr/bin/env bash
set -euo pipefail

# clean_main_repo_history.sh
#
# WARNING: This script rewrites git history and optionally force-pushes the cleaned repo.
# Only run this when you fully understand the consequences. Because you are currently the
# only contributor, this may be acceptable, but still: make backups.
#
# Requirements:
# - git-filter-repo installed (https://github.com/newren/git-filter-repo)
# - A local or remote location to push the rewritten repo (this script will push to the
#   same origin by default, using --force).
#
# Typical usage (safe):
#  # Create a mirror clone and test the filter
#  ./scripts/clean_main_repo_history.sh --mirror-dir /tmp/ccos-mirror --dry-run
#
# Real run (force push):
#  ./scripts/clean_main_repo_history.sh --mirror-dir /tmp/ccos-mirror --force-push
#
usage() {
  cat <<'USAGE'
Usage: clean_main_repo_history.sh [options]

Options:
  --mirror-dir DIR     Directory to create the mirror clone in (required)
  --preserve-refs      Comma separated refs to preserve (default: none)
  --force-push         After cleaning, force-push cleaned refs to origin (dangerous)
  --dry-run            Print commands that would be run and stop
  --help               Show this help

This script will:
  1. Create a mirror clone of the current repository into --mirror-dir
  2. Run git-filter-repo to remove the 'chats/' path from history
  3. Run garbage collection and pruning
  4. Optionally force-push the cleaned repo back to origin (replace remote history)

Be sure to back up the mirror-dir before force-pushing.
USAGE
}

MIRROR_DIR=""
FORCE_PUSH="no"
DRY_RUN="no"
PRESERVE_REFS=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mirror-dir) MIRROR_DIR="$2"; shift 2;;
    --force-push) FORCE_PUSH="yes"; shift;;
    --dry-run) DRY_RUN="yes"; shift;;
    --preserve-refs) PRESERVE_REFS="$2"; shift 2;;
    --help) usage; exit 0;;
    *) echo "Unknown arg: $1"; usage; exit 1;;
  esac
done

if [ -z "$MIRROR_DIR" ]; then
  echo "--mirror-dir is required" >&2
  usage
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Mirror directory: $MIRROR_DIR"
echo "Repository root: $REPO_ROOT"

if [ "$DRY_RUN" = "yes" ]; then
  echo "DRY RUN: The script will show actions and exit."
fi

# Step 1: create mirror clone
if [ "$DRY_RUN" = "yes" ]; then
  echo "Would run: git clone --mirror $REPO_ROOT $MIRROR_DIR"
else
  rm -rf "$MIRROR_DIR"
  git clone --mirror "$REPO_ROOT" "$MIRROR_DIR"
fi

cd "$MIRROR_DIR"

# Safety check: ensure we have a bare repo
if [ ! -f "config" ]; then
  echo "Mirror clone does not look valid" >&2
  exit 1
fi

# Step 2: run git-filter-repo to remove chats/
FILTER_CMD=(git filter-repo --invert-paths --path chats --force)

if [ -n "$PRESERVE_REFS" ]; then
  echo "Note: preserve refs was requested but not implemented in this template: $PRESERVE_REFS"
fi

if [ "$DRY_RUN" = "yes" ]; then
  echo "Would run: ${FILTER_CMD[*]}"
  echo "After verifying changes you can run the script without --dry-run to perform the rewrite."
  exit 0
fi

echo "Running: ${FILTER_CMD[*]}"
"${FILTER_CMD[@]}"

# Step 3: gc and clean
echo "Running git reflog expire --expire=now --all && git gc --prune=now --aggressive"
git reflog expire --expire=now --all
git gc --prune=now --aggressive

echo "Local mirror cleaned. You should inspect the mirror repo in $MIRROR_DIR before force-pushing."

if [ "$FORCE_PUSH" = "yes" ]; then
  echo "Force push requested. This will overwrite origin with the cleaned history."
  read -p "Type YES to continue: " answer
  if [ "$answer" != "YES" ]; then
    echo "Aborting force-push."
    exit 1
  fi
  echo "Pushing cleaned refs to origin (force)..."
  git push --mirror --force
  echo "Force-push done. You may need to contact GitHub to purge orphaned LFS objects from storage."
else
  echo "Force push not requested. If you want to publish the cleaned repo, re-run with --force-push after verifying the mirror." 
fi

echo "Script complete. Backup your mirror before doing any destructive operation."
