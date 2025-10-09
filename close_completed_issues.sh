#!/bin/bash
# Script to close completed GitHub issues
# This script requires GITHUB_TOKEN to be set

set -e

REPO="mandubian/ccos"

# Function to close an issue with a comment
close_issue() {
    local issue_number=$1
    local comment=$2
    
    echo "Closing issue #${issue_number}..."
    
    # Add closing comment
    gh issue comment $issue_number --repo $REPO --body "$comment"
    
    # Close the issue
    gh issue close $issue_number --repo $REPO --reason completed
    
    echo "✅ Closed issue #${issue_number}"
}

# Issue #115 - RTFS Stability Umbrella
close_issue 115 "Closing Issue #115 as all tracked work is complete:
- ✅ #109: Missing stdlib helpers (sort, frequencies, etc.) - CLOSED
- ✅ #110: 4-arg update semantics - CLOSED
- ✅ #111: Undefined symbol failures - CLOSED
- ✅ #112: Host execution context fixes - CLOSED
- ✅ #113: IR assign/set! support - CLOSED
- ✅ #114: Parser map-type braced acceptance - CLOSED

All issues have been resolved and merged. Documentation exists in \`docs/archive/rtfs-2.0/specs-before-migration/RTFS_STABILITY_IMPLEMENTATION_SUMMARY.md\`."

# Issue #23 - Arbiter V1
close_issue 23 "Closing Issue #23 as Arbiter V1 is feature-complete. Per \`docs/archive/rtfs_compiler/ISSUE_23_COMPLETION_REPORT.md\`:

**Completed Milestones**:
- ✅ M1: LLM execution bridge with security gating
- ✅ M2: NL→Intent & Plan conversion (templates + LLM)
- ✅ M3: Dynamic capability resolution via marketplace
- ✅ M4: Agent Registry & Delegation Lifecycle
- ✅ M5: Task Protocol bootstrap (TaskRequest/TaskResult)

**Implementation Evidence**:
- \`rtfs_compiler/src/ccos/arbiter/\` - Complete implementation
- \`rtfs_compiler/src/ccos/delegation.rs\` - Delegation logic
- \`rtfs_compiler/src/ccos/task_protocol.rs\` - Task protocol structures

Follow-up enhancements tracked in separate issues (#91-96)."

# Issue #79 - Execution Context Management
close_issue 79 "Closing Issue #79 as Phase 1 of hierarchical execution context management is complete. Per \`docs/archive/rtfs_compiler/ISSUE_79_COMPLETION_REPORT.md\`:

**Delivered**:
- ✅ Hierarchical context stack with parent/child relationships
- ✅ Isolation levels (Inherit, Isolated, Sandboxed)
- ✅ Evaluator integration (step, step-if, step-loop, step-parallel)
- ✅ Checkpoint/resume interface
- ✅ Documentation in SEP-015
- ✅ Comprehensive tests

**Implementation**:
- \`rtfs_compiler/src/ccos/execution_context.rs\`
- \`rtfs_compiler/src/ccos/context_horizon.rs\`
- Tests: \`execution_context_tests.rs\`, \`orchestrator_checkpoint_tests.rs\`"

# Issue #86 - Progress Update
close_issue 86 "Closing Issue #86 as it was a progress update for Issue #79 (Execution Context Management), which is now complete. All work tracked here has been delivered in #79."

# Issues #81-85 - Arbiter Milestones
for issue in 81 82 83 84 85; do
    milestone=""
    case $issue in
        81) milestone="M1 (LLM execution bridge)" ;;
        82) milestone="M2 (NL→Intent/Plan conversion)" ;;
        83) milestone="M3 (Dynamic capability resolution)" ;;
        84) milestone="M4 (Agent registry integration)" ;;
        85) milestone="M5 (Task delegation protocol)" ;;
    esac
    
    close_issue $issue "Closing Issue #${issue} as it was ${milestone} of Issue #23 (Arbiter V1), which is now complete. All functionality has been delivered and is documented in \`docs/archive/rtfs_compiler/ISSUE_23_COMPLETION_REPORT.md\`."
done

# Additional issues discovered during investigation
echo "Closing additional completed issues..."

# Issue #80 - Orchestrator tracking (children #13, #78, #79 all closed)
close_issue 80 "Closing Issue #80 as all tracked critical features are now implemented:
- ✅ #78: Advanced Orchestration Primitives (step.if, step.loop, step.parallel) - CLOSED
- ✅ #79: Hierarchical Execution Context Management - CLOSED  
- ✅ #13: Action execution with parameter binding - CLOSED

All orchestration primitives and execution context features have been delivered. Remaining features (Plan Archive, enhanced Causal Chain, resource management) are tracked in separate active issues."

# Issue #120 - Tracking/organizational issue (worktrees completed, served its purpose)
close_issue 120 "Closing Issue #120 as a tracking/organizational issue that has served its purpose. The worktrees mentioned (\`wt/rtfs-stability-core\`, \`wt/orch-core-status-context\`) have been completed and merged. Active work is now tracked in specific feature issues rather than worktree-based organization."

echo ""
echo "✅ Successfully closed 11 completed issues!"
echo ""
echo "Summary of closed issues:"
echo "- #23: Arbiter V1 (feature complete)"
echo "- #79: Execution Context Management (Phase 1 complete)"
echo "- #80: Orchestrator tracking (children #13, #78, #79 closed)"
echo "- #81-85: Arbiter V1 milestones M1-M5"
echo "- #86: Progress update for #79"
echo "- #115: RTFS Stability Umbrella (all children closed)"
echo "- #120: Tracking/organizational (worktrees completed)"
