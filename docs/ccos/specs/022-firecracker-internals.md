# MicroVM Architecture for Sandboxed Execution

## Overview

CCOS uses MicroVMs (specifically AWS Firecracker) to provide hardware-level isolation for executing untrusted code. This document explains the architecture, the one-shot execution model, and the technical challenges we solved.

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────────┐
│                      CCOS Runtime                                │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              MicroVMProvider Trait                         │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────────────┐  │  │
│  │  │Firecrack│ │ gVisor  │ │  WASM   │ │ Process (fallbk)│  │  │
│  │  │   er    │ │         │ │         │ │                 │  │  │
│  │  └────┬────┘ └─────────┘ └─────────┘ └─────────────────┘  │  │
│  └───────┼───────────────────────────────────────────────────┘  │
│          │                                                       │
│          ▼                                                       │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              ScriptLanguage Enum                           │  │
│  │  Python | JavaScript | Shell | Ruby | Lua | Custom         │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Firecracker MicroVM                           │
│  ┌────────────┐    ┌─────────────────────────────────────────┐  │
│  │   Kernel   │    │           Modified rootfs.ext4           │  │
│  │ vmlinux    │    │  ┌─────────────┐  ┌─────────────────┐   │  │
│  │            │    │  │ /rtfs_init  │  │ /rtfs_script.py │   │  │
│  │            │    │  │ (init=)     │  │ (user code)     │   │  │
│  └────────────┘    │  └─────────────┘  └─────────────────┘   │  │
│                    └─────────────────────────────────────────┘  │
│  Serial Console (stdout) ◄──── Output with markers              │
└─────────────────────────────────────────────────────────────────┘
```

## Key Components

### 1. ScriptLanguage Enum

Defines supported languages for sandboxed execution:

```rust
pub enum ScriptLanguage {
    Python,      // /usr/bin/python, python3, python2
    JavaScript,  // /usr/bin/node
    Shell,       // /bin/sh, /bin/bash
    Ruby,        // /usr/bin/ruby
    Lua,         // /usr/bin/lua
    Rtfs,        // Custom RTFS interpreter
    Custom { interpreter: String, file_ext: String },
}
```

Each language provides:
- `interpreter()` - Primary interpreter path
- `file_extension()` - File extension for script files
- `interpreter_alternatives()` - Fallback paths to try

### 2. Program Enum

```rust
pub enum Program {
    RtfsSource(String),           // Legacy RTFS source
    ScriptSource {                // NEW: Explicit language
        language: ScriptLanguage,
        source: String,
    },
    ExternalProgram { path, args },
    RtfsBytecode(Vec<u8>),
    RtfsAst(Box<Expression>),
    NativeFunction(fn),
}
```

### 3. One-Shot VM Execution Model

Unlike long-running VMs, we use a **one-shot execution model**:

1. Create temporary overlay rootfs
2. Inject script + custom init
3. Boot VM with `init=/rtfs_init`
4. Capture output via serial console
5. VM self-terminates after execution
6. Clean up temporary files

This approach:
- Eliminates state persistence between executions
- Provides perfect isolation
- Simplifies resource management
- ~2 seconds per execution

## Filesystem Injection Mechanism

### The Challenge

Firecracker boots a Linux kernel with an ext4 rootfs. We need to:
1. Inject user code into the filesystem
2. Replace `/sbin/init` with our custom init script
3. Preserve correct file permissions

### Solution: debugfs + Overlay

```
Base rootfs.ext4 ──► Copy to /tmp/overlay ──► debugfs inject ──► Boot
     (read-only)         (writable copy)        (add files)
```

#### Step 1: Create Overlay Directory
```rust
let work_dir = PathBuf::from(format!("/tmp/fc-overlay-{}", vm_id));
fs::create_dir_all(&work_dir)?;
```

#### Step 2: Write Script Files
```rust
// User's script
let script_filename = format!("rtfs_script.{}", language.file_extension());
fs::write(&script_path, script_source)?;

// Custom init script
fs::write(&init_path, &init_script)?;
```

#### Step 3: Set Executable Permissions (CRITICAL!)
```rust
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&init_path)?.permissions();
    perms.set_mode(0o755);  // rwxr-xr-x
    fs::set_permissions(&init_path, perms)?;
}
```

**Why this matters:** `debugfs write` preserves source file permissions. If the source file is 0644 (rw-r--r--), the injected file will also be 0644 and the kernel will fail to execute it as init.

#### Step 4: Copy Base Rootfs
```bash
cp /opt/firecracker/rootfs.ext4 /tmp/fc-overlay-{vm_id}/rootfs.ext4
```

#### Step 5: Inject Files with debugfs
```bash
debugfs -w -R "write /path/to/script.py rtfs_script.py" rootfs.ext4
debugfs -w -R "write /path/to/rtfs_init rtfs_init" rootfs.ext4
```

### Permission Issues We Encountered

#### Error 1: "bogus i_mode (755)"
**Cause:** Using `set_inode_field` with decimal mode value instead of octal.
```bash
# WRONG: 755 decimal = 01363 octal (invalid mode)
debugfs -R "set_inode_field rtfs_init mode 755"

# RIGHT: Use octal notation
debugfs -R "set_inode_field rtfs_init mode 0755"
```

#### Error 2: "Kernel panic - error -117 (EUCLEAN)"
**Cause:** Filesystem corruption from incorrect inode manipulation.
**Solution:** Don't use `set_inode_field` at all. Instead, set permissions on the source file BEFORE injection.

#### Error 3: Init script not executable
**Cause:** Source file had 0644 permissions, which were preserved during injection.
**Solution:** `chmod 755` on source file before `debugfs write`.

### Final Working Flow

```
1. Write script.py to host filesystem
2. Write rtfs_init to host filesystem  
3. chmod 755 rtfs_init on host         ◄── CRITICAL STEP
4. Copy rootfs.ext4 to overlay
5. debugfs write script.py → rootfs    (preserves source perms)
6. debugfs write rtfs_init → rootfs    (preserves 0755 perms)
7. Boot VM with init=/rtfs_init
```

## Custom Init Script

The init script replaces `/sbin/init` and performs:

```bash
#!/bin/sh
# Mount essential filesystems
mount -t proc proc /proc 2>/dev/null
mount -t sysfs sysfs /sys 2>/dev/null

# Output markers for parsing
echo "===RTFS_OUTPUT_START==="

# Dynamic interpreter selection
if [ -x /usr/bin/python ]; then
    /usr/bin/python /rtfs_script.py 2>&1
elif [ -x /usr/bin/python3 ]; then
    /usr/bin/python3 /rtfs_script.py 2>&1
elif [ -x /usr/bin/python2 ]; then
    /usr/bin/python2 /rtfs_script.py 2>&1
else
    echo "ERROR: No Python interpreter found"
fi

EXIT_CODE=$?
echo "===RTFS_OUTPUT_END==="
echo "===RTFS_EXIT_CODE===:$EXIT_CODE"

# Trigger immediate poweroff
sync
echo 1 > /proc/sys/kernel/sysrq 2>/dev/null
echo o > /proc/sysrq-trigger 2>/dev/null
```

Key features:
- **Output markers** - Allow parsing script output from kernel boot noise
- **Dynamic interpreter detection** - Works with different rootfs images
- **Exit code capture** - Propagates script exit status
- **Clean shutdown** - Uses sysrq-trigger for immediate poweroff

## Output Parsing

Serial console output contains kernel messages mixed with script output:

```
[    0.000000] Linux version 4.14.174...
[    0.123456] ... (many kernel messages) ...
===RTFS_OUTPUT_START===
{"message": "Hello from Firecracker!", "calculated": 4}
===RTFS_OUTPUT_END===
===RTFS_EXIT_CODE===:0
[    0.856757] random: fast init done
```

Parsing logic:
```rust
if let Some(start_idx) = stdout.find(OUTPUT_START_MARKER) {
    let after_start = start_idx + OUTPUT_START_MARKER.len();
    if let Some(end_offset) = stdout[after_start..].find(OUTPUT_END_MARKER) {
        let script_output = &stdout[after_start..after_start + end_offset];
        return script_output.trim().to_string();
    }
}
```

## Non-Blocking I/O

### The Problem

Child process stdout/stderr must be read without blocking indefinitely:
1. Stdout may not have EOF until process exits
2. Stderr read_to_end() blocks while process is running
3. Need to detect output markers before process terminates

### Solution: fcntl O_NONBLOCK

```rust
use std::os::unix::io::AsRawFd;

let fd = stdout.as_raw_fd();
unsafe {
    let flags = libc::fcntl(fd, libc::F_GETFL);
    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
}
```

### Read Loop Pattern

```rust
loop {
    if start.elapsed() > timeout {
        break;  // Timeout protection
    }
    
    match child.try_wait() {
        Ok(Some(_)) => { /* Process exited, drain remaining */ }
        Ok(None) => { /* Still running */ }
        Err(_) => break,
    }
    
    match stdout.read(&mut buf) {
        Ok(0) => break,  // EOF
        Ok(n) => {
            data.extend_from_slice(&buf[..n]);
            if data.contains(END_MARKER) {
                break;  // Got complete output
            }
        }
        Err(e) if e.kind() == WouldBlock => {
            sleep(100ms);  // No data yet, wait
        }
        Err(_) => break,
    }
}

// IMPORTANT: Kill process BEFORE reading stderr
child.kill()?;
child.wait()?;

// Now safe to read stderr (process is dead)
stderr.read_to_end(&mut stderr_data)?;
```

## Firecracker API

Communication with Firecracker uses a Unix socket with HTTP/1.1:

```rust
let socket = UnixStream::connect("/tmp/fc-{vm_id}.sock")?;

// Configure boot source
PUT /boot-source
{
    "kernel_image_path": "/opt/firecracker/vmlinux",
    "boot_args": "console=ttyS0 reboot=k panic=1 pci=off init=/rtfs_init"
}

// Configure root drive
PUT /drives/rootfs
{
    "drive_id": "rootfs",
    "path_on_host": "/tmp/fc-overlay-{id}/rootfs.ext4",
    "is_root_device": true,
    "is_read_only": false
}

// Configure machine
PUT /machine-config
{
    "vcpu_count": 1,
    "mem_size_mib": 128,
    "smt": false
}

// Start VM
PUT /actions
{"action_type": "InstanceStart"}
```

## Requirements

### System Requirements
- Linux with KVM enabled (`/dev/kvm` accessible)
- Firecracker binary (`/opt/firecracker/firecracker`)
- Linux kernel (`/opt/firecracker/vmlinux`)
- Base rootfs image (`/opt/firecracker/rootfs.ext4`)
- `debugfs` tool (from e2fsprogs package)

### Rootfs Requirements
The rootfs.ext4 must contain:
- Interpreter binaries (e.g., `/usr/bin/python`)
- Essential libraries for the interpreters
- Basic filesystem structure (`/proc`, `/sys` mount points)

### Current Rootfs
We use an Ubuntu 18.04 minimal rootfs (~300MB) with:
- Python 2.7 (`/usr/bin/python`, `/usr/bin/python2`)
- Basic shell utilities

## Performance

Typical execution times:
- VM startup: ~800ms
- Script execution: depends on script
- Total round-trip: ~2 seconds

Optimizations possible:
- Pre-warmed VM pool (not yet implemented)
- Smaller rootfs images
- Faster copy methods (reflink, overlay filesystem)

## Security Considerations

1. **Isolation**: Full hardware virtualization via KVM
2. **No network**: Network disabled by default
3. **Read-only base**: Base rootfs not modified
4. **Ephemeral**: Each execution uses fresh overlay
5. **Resource limits**: CPU/memory limits via Firecracker config
6. **No persistence**: VM destroyed after execution

## Error Handling

Common failure modes:
- `ENOENT` - Missing kernel/rootfs/firecracker binary
- `EPERM` - No access to /dev/kvm
- `ENOSPC` - /tmp full (overlay files)
- `ETIMEDOUT` - Script execution timeout
- `EUCLEAN` - Filesystem corruption (debugfs error)

## Future Improvements

1. **VM Pool**: Pre-started VMs for faster execution
2. **vsock Communication**: Replace serial console with vsock
3. **Network Isolation**: Optional network with firewall
4. **More Languages**: Node.js, Ruby, Lua in rootfs
5. **Custom Rootfs**: Per-language optimized images
6. **Jailer Integration**: Additional security hardening
