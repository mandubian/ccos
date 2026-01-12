#!/bin/bash
set -e

# setup_worktree.sh - Initialize a new CCOS worktree
# This script focuses on directory parity and build performance.

echo "🚀 Initializing CCOS worktree: $(pwd)"

# 1. Create untracked directories required by the CCOS runtime
# These are ignored by git but expected by agent_config.toml.
echo "📂 Creating runtime directories..."
mkdir -p capabilities/core \
         capabilities/servers/pending \
         capabilities/servers/approved \
         capabilities/generated \
         capabilities/learned \
         capabilities/samples \
         capabilities/web \
         capabilities/sessions \
         storage/approvals \
         storage/pending_synth \
         storage/plans \
         config \
         config/storage \
         demo_storage/intents \
         demo_storage/plans \
         demo_storage/edges \
         logs \
         generated_capabilities

# 2. Shared Build Cache (Optional but recommended for worktrees)
# To avoid duplicating 5GB+ of target artifacts per worktree, 
# you can point CARGO_TARGET_DIR to a shared location in your terminal.
# If not already set, this script creates a local one.
if [ -z "$CARGO_TARGET_DIR" ]; then
    echo "ℹ️  CARGO_TARGET_DIR not set. Using local ./target"
else
    echo "🏗️  Using shared build cache: $CARGO_TARGET_DIR"
fi

# 3. Warm the Cargo Cache
# Ensures all workspace members (rtfs, ccos, viewer_server) are resolvable.
echo "📦 Fetching dependencies..."
cargo fetch --workspace

# 4. Verify Worktree Integrity
# A quick check to ensure the toolchain and dependencies are correctly linked.
echo "🔍 Running workspace check..."
cargo check --workspace

echo "✅ Setup complete. Environment variables are assumed to be inherited from the shell."