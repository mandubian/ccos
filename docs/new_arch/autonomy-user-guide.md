# CCOS Autonomy User Guide

This guide describes how to use the autonomy features in CCOS, including scheduling, checkpointing, and agent jailing.

## Installation: bubblewrap

`bubblewrap` (`bwrap`) is required for agent jailing on Linux. It provides a lightweight, unprivileged sandbox using Linux namespaces.

### How to Install

Depending on your distribution, run the following command:

- **Ubuntu / Debian**:
  ```bash
  sudo apt update && sudo apt install bubblewrap
  ```

- **Fedora / RHEL / CentOS**:
  ```bash
  sudo dnf install bubblewrap
  ```

- **Arch Linux**:
  ```bash
  sudo pacman -S bubblewrap
  ```

- **openSUSE**:
  ```bash
  sudo zypper install bubblewrap
  ```

### Verifying Installation
Run `bwrap --version` to verify it is installed and in your PATH.

---

## Feature: Agent Jailing

Agent jailing isolates agent processes from the host system, preventing direct network access and unauthorized filesystem interaction.

### Configuration
Enable jailing by setting the following environment variable for the Gateway:
```bash
export CCOS_GATEWAY_JAIL_AGENTS=true
```

When enabled, the Gateway uses `JailedProcessSpawner` which wraps the agent in a `bwrap` sandbox. All agent I/O is routed through the Gateway APIs.

---

## Feature: Cron Scheduler

The scheduler allows you to run autonomous goals on a recurring or delayed basis.

### Usage
When creating a run via `POST /chat/run`, you can provide an optional `schedule` parameter (cron expression).

**Example Request**:
```json
{
  "goal": "Check Moltbook feed every hour",
  "schedule": "0 0 * * * *",
  "meta": {
    "session_id": "sess-autonomy-demo"
  }
}
```

The Gateway will automatically spawn an agent at the scheduled times.

---

## Feature: Checkpoint & Resume

Checkpoints allow agents to persist their logical state (Working Memory, instruction pointer) so they can resume precisely where they left off after a restart or segment boundary.

### Manual Checkpoint
You can trigger a checkpoint for an active run:
```http
POST /chat/run/:run_id/checkpoint
```

### Resume
You can resume a paused or checkpointed run:
```http
POST /chat/run/:run_id/resume
```

### Implementation Details
Checkpoints are stored in the Gateway's internal store and recorded to the **Causal Chain**, making them durable across Gateway restarts.
