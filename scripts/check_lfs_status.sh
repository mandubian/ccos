#!/usr/bin/env bash
set -euo pipefail

# check_lfs_status.sh
# 
# Check which chat files are available locally vs LFS pointers.
# Helps determine if migration can proceed or if files need to be fetched first.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CHATS_DIR="$REPO_ROOT/chats"

echo "Checking LFS status of chat files..."
echo "Repository: $REPO_ROOT"
echo "Chats directory: $CHATS_DIR"
echo ""

if [ ! -d "$CHATS_DIR" ]; then
  echo "Error: chats/ directory not found" >&2
  exit 1
fi

# Count expected LFS files
echo "=== LFS File Counts ==="
EXPECTED_LFS=$(git lfs ls-files | grep -c "chats/" || echo "0")
echo "Expected LFS files (chats/): $EXPECTED_LFS"
echo ""

# Check JSON files
echo "=== JSON File Status ==="
JSON_FILES=$(find "$CHATS_DIR" -maxdepth 1 -name "*.json" | sort)
JSON_COUNT=$(echo "$JSON_FILES" | grep -c . || echo "0")
echo "JSON files found: $JSON_COUNT"
echo ""

POINTER_COUNT=0
CONTENT_COUNT=0
TOTAL_SIZE=0
POINTER_FILES=()
CONTENT_FILES=()

for f in $JSON_FILES; do
  if [ ! -f "$f" ]; then
    continue
  fi
  
  # Check if it's an LFS pointer
  if head -1 "$f" 2>/dev/null | grep -q "version https://git-lfs.github.com/spec/v1"; then
    POINTER_COUNT=$((POINTER_COUNT + 1))
    POINTER_FILES+=("$(basename "$f")")
    echo "  POINTER: $(basename "$f") ($(stat -c%s "$f" 2>/dev/null || echo "0") bytes)"
  else
    CONTENT_COUNT=$((CONTENT_COUNT + 1))
    SIZE=$(stat -c%s "$f" 2>/dev/null || echo "0")
    TOTAL_SIZE=$((TOTAL_SIZE + SIZE))
    CONTENT_FILES+=("$(basename "$f")")
    SIZE_MB=$(echo "scale=2; $SIZE / 1048576" | bc 2>/dev/null || echo "?")
    echo "  CONTENT: $(basename "$f") (${SIZE_MB} MB)"
  fi
done

echo ""
echo "=== Summary ==="
echo "JSON files with actual content: $CONTENT_COUNT"
echo "JSON files that are LFS pointers: $POINTER_COUNT"
echo "Total content size: $(echo "scale=2; $TOTAL_SIZE / 1073741824" | bc 2>/dev/null || echo "?") GB"
echo ""

# Check MD files too
MD_FILES=$(find "$CHATS_DIR" -maxdepth 1 -name "*.md" | wc -l)
echo "MD files found: $MD_FILES"

echo ""
echo "=== Migration Readiness ==="
if [ "$POINTER_COUNT" -eq 0 ]; then
  echo "✅ All JSON files have content locally - ready to migrate"
elif [ "$CONTENT_COUNT" -eq 0 ]; then
  echo "❌ No JSON files have content - all are pointers"
  echo "   Run: git lfs pull --include=\"chats/*\" to fetch files"
else
  echo "⚠️  Partial availability: $CONTENT_COUNT files ready, $POINTER_COUNT files need fetching"
  echo "   Options:"
  echo "   1. Fetch missing: git lfs pull --include=\"chats/*\""
  echo "   2. Migrate only available files (skip pointers)"
  echo "   3. Archive pointers as-is (uncompressed)"
fi

if [ "$POINTER_COUNT" -gt 0 ]; then
  echo ""
  echo "=== Files That Need Fetching ==="
  for f in "${POINTER_FILES[@]}"; do
    echo "  - $f"
  done
fi

