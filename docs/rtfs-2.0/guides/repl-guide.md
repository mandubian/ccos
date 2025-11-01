# RTFS Interactive REPL Guide

**Version**: 2.0  
**User-Friendly** • **Type-Safe** • **Beginner-Friendly**

---

## Getting Started

### Launch the REPL

```bash
cargo run --package rtfs --bin rtfs-repl --features repl
```

You'll see:
```
╔══════════════════════════════════════════════════════╗
║  🚀 RTFS Interactive REPL v2.0                      ║
║  Type-Safe • Interactive • Developer-Friendly       ║
╚══════════════════════════════════════════════════════╝

💡 Quick Tips:
  • Type expressions and see results instantly
  • Use :commands for special features (type :help for list)
  • Multi-line: paste code, press Enter twice
  • Type checking is ON by default for safety

rtfs>
```

---

## Interactive Commands

All commands start with `:` for easy discovery.

### 🔍 `:type` - See What Type Your Code Is

**Usage**: Type an expression, then `:type`

```
rtfs> (+ 1 2.5)
╭─────────────────────────────────────╮
│ ✅ Result: Float(3.5)
╰─────────────────────────────────────╯

rtfs> :type
┌─ 🔍 TYPE INFORMATION ─────────────────────────────────┐
│ Expression 1: Number (Int or Float)
│ 📘 This is a Number (can be Int or Float)
└────────────────────────────────────────────────────────┘
```

**Perfect for beginners**: Shows types in plain English!

---

### 🌳 `:ast` - See How RTFS Understands Your Code

**Usage**: Type code, then `:ast`

```
rtfs> [1 2.5 3]
╭─────────────────────────────────────╮
│ ✅ Result: Vector([Integer(1), Float(2.5), Integer(3)])
╰─────────────────────────────────────╯

rtfs> :ast
┌─ 🌳 SYNTAX TREE (How RTFS Sees Your Code) ───────────┐
│
│ Expression 1:
│   └─ 📦 Vector [3 items]
│      │├─ 💎 Integer(1)
│      │├─ 💎 Float(2.5)
│       └─ 💎 Integer(3)
└────────────────────────────────────────────────────────┘
```

**Visual tree structure** - easy to understand!

---

### 💭 `:explain` - Get Plain Language Explanation

**Usage**: Type code, then `:explain`

```
rtfs> (+ 1 2 3)
╭─────────────────────────────────────╮
│ ✅ Result: Integer(6)
╰─────────────────────────────────────╯

rtfs> :explain
┌─ 💭 CODE EXPLANATION ────────────────────────────────┐
│
│ Expression 1:
│   This adds 3 numbers together
│   With 3 argument(s)
│
│ 📘 Type: Number (Int or Float)
└────────────────────────────────────────────────────────┘
```

**No jargon** - explains what the code actually does!

---

### 🔒 `:security` - Check If Code Is Safe

**Usage**: Type code, then `:security`

```
rtfs> (+ 1 2)
rtfs> :security
┌─ 🔒 SECURITY ANALYSIS ───────────────────────────────┐
│ ✅ Safe: No external operations detected
│ 📘 This code is pure (no side effects)
└────────────────────────────────────────────────────────┘

rtfs> (read-file "/etc/passwd")
rtfs> :security
┌─ 🔒 SECURITY ANALYSIS ───────────────────────────────┐
│ ⚠️  External Operations Detected:
│   • File I/O: read-file
│
│ 🔐 Security Level:
│   Controlled (file access monitored)
└────────────────────────────────────────────────────────┘
```

**Instant security feedback** - know what your code does!

---

### 📊 `:info` - Complete Analysis (One Command!)

**Usage**: Type code, then `:info`

```
rtfs> (+ 1 2.5)
rtfs> :info

╔══════════════════════════════════════════════════════╗
║  📊 COMPREHENSIVE CODE ANALYSIS                     ║
╚══════════════════════════════════════════════════════╝

🔸 Expression 1:
  Input: (+ 1 2.5)

  🔍 Type: Number (Int or Float)
  ✅ Type Check: PASS

  📈 Complexity:
    IR nodes: 4 → 4 (after optimization)

  🔒 Security:
    ✅ Pure (no external operations)

  ⏱️  Analysis Time: 186µs
```

**Everything at once** - type, security, complexity, timing!

---

### 🔧 `:ir` - See Optimized Internal Representation

**Usage**: Type code, then `:ir`

For advanced users who want to see the optimized intermediate representation.

---

### ✨ `:format` - Prettify Your Code

**Usage**: Type code, then `:format`

```
rtfs> (+ 1 2 3)
rtfs> :format
┌─ ✨ FORMATTED CODE ──────────────────────────────────┐
│ (+ 1 2 3)
└────────────────────────────────────────────────────────┘
```

---

## Auto-Display Settings

Turn on auto-display for types, IR, or timing!

### ⚙️ `:set types on` - Always Show Types

```
rtfs> :set types on
✅ Type display: ON

rtfs> (+ 1 2)
🔍 Type: Number (Int or Float)
╭─────────────────────────────────────╮
│ ✅ Result: Integer(3)
╰─────────────────────────────────────╯
```

**Perfect for learning** - see types as you go!

### ⚙️ `:set timing on` - Always Show Performance

```
rtfs> :set timing on
✅ Timing display: ON

rtfs> (+ 1 2 3 4 5)
╭─────────────────────────────────────╮
│ ✅ Result: Integer(15)
│ ⏱️  Time: 142µs
╰─────────────────────────────────────╯
```

### ⚙️ `:set` - Show Current Settings

```
rtfs> :set
📝 Current Settings:
  show_types: true
  show_ir: false
  show_timing: true

ℹ️  Usage: :set <option> <on|off>
  Options: types, ir, timing
```

---

## Multi-Line Input

### Automatic Bracket Balancing

```
rtfs> (let [x 10]
  (+ x 20))
       
```

Press Enter twice when done. RTFS automatically detects balanced brackets!

### Visual Feedback

```
rtfs> (let [x 10]
  ↳   (+ x 20))
  ↳
```

The `↳` prompt shows you're in multi-line mode.

---

## Example Sessions

### Session 1: Learning Types

```
rtfs> [1 2 3]
╭─────────────────────────────────────╮
│ ✅ Result: Vector([Integer(1), Integer(2), Integer(3)])
╰─────────────────────────────────────╯

rtfs> :type
┌─ 🔍 TYPE INFORMATION ─────────────────────────────────┐
│ Expression 1: Vector of Integer
└────────────────────────────────────────────────────────┘

rtfs> [1 2.5 3]
╭─────────────────────────────────────╮
│ ✅ Result: Vector([Integer(1), Float(2.5), Integer(3)])
╰─────────────────────────────────────╯

rtfs> :type
┌─ 🔍 TYPE INFORMATION ─────────────────────────────────┐
│ Expression 1: Vector of Number (Int or Float)
│ 📘 This is a Vector containing: Number (Int or Float)
└────────────────────────────────────────────────────────┘
```

### Session 2: Understanding Code

```
rtfs> (+ 10 20 30)
╭─────────────────────────────────────╮
│ ✅ Result: Integer(60)
╰─────────────────────────────────────╯

rtfs> :explain
┌─ 💭 CODE EXPLANATION ────────────────────────────────┐
│
│ Expression 1:
│   This adds 3 numbers together
│   With 3 argument(s)
│
│ 📘 Type: Number (Int or Float)
└────────────────────────────────────────────────────────┘
```

### Session 3: Security Analysis

```
rtfs> (+ 1 2)
rtfs> :security
┌─ 🔒 SECURITY ANALYSIS ───────────────────────────────┐
│ ✅ Safe: No external operations detected
│ 📘 This code is pure (no side effects)
└────────────────────────────────────────────────────────┘

rtfs> (http-fetch "https://example.com")
rtfs> :security
┌─ 🔒 SECURITY ANALYSIS ───────────────────────────────┐
│ ⚠️  External Operations Detected:
│   • Network: http-fetch
│
│ 🔐 Security Level:
│   Sandboxed (requires MicroVM)
│   📘 Network operations need strict isolation
└────────────────────────────────────────────────────────┘
```

---

## Comparison: REPL vs Compiler

| Feature | REPL | Compiler |
|---------|------|----------|
| **Interactive** | ✅ Live feedback | ❌ Batch mode |
| **User-Friendly** | ✅ Plain language | ⚠️ Technical |
| **Visual Output** | ✅ Box drawings | ⚠️ Debug format |
| **Commands** | ✅ `:type`, `:explain`, etc. | ⚠️ CLI flags only |
| **Auto-Display** | ✅ `:set types on` | ❌ Manual flags |
| **Learning** | ✅ Perfect for beginners | ⚠️ For experts |
| **Production** | ⚠️ Development tool | ✅ Build tool |

---

## Tips for Beginners

### 1. Start with `:set types on`

See types as you learn:
```
rtfs> :set types on
rtfs> 42
🔍 Type: Integer
╭─────────────────────────────────────╮
│ ✅ Result: Integer(42)
╰─────────────────────────────────────╯

rtfs> 3.14
🔍 Type: Float
╭─────────────────────────────────────╮
│ ✅ Result: Float(3.14)
╰─────────────────────────────────────╯
```

### 2. Use `:explain` When Confused

```
rtfs> (/ 10 3)
rtfs> :explain
│   This divides numbers
│   With 2 argument(s)
│
│ 📘 Type: Number (Int or Float)
```

### 3. Check Security Before Running

```
rtfs> (write-file "output.txt" "data")
rtfs> :security
│ ⚠️  External Operations Detected:
│   • File I/O: write-file
```

### 4. Use `:info` for Everything

One command shows: type, security, complexity, timing!

---

## Advanced Features

### Command Shortcuts

All commands have short forms:
- `:h` = `:help`
- `:t` = `:type`
- `:e` = `:explain`
- `:i` = `:info`
- `:sec` = `:security`
- `:fmt` = `:format`

### Persistent State

The REPL remembers:
- Last expression evaluated
- Last result
- Settings (types, ir, timing)

This means `:type`, `:explain`, etc. work on your previous input!

### Non-Interactive Mode

Use the REPL in scripts:

```bash
# From string
rtfs-repl --input string --string '(+ 1 2 3)'

# From file
rtfs-repl --input file --file mycode.rtfs

# From pipe
echo '(+ 1 2)' | rtfs-repl --input pipe
```

---

## What Makes This User-Friendly?

### 1. Plain Language

❌ Technical: `Union([Int, Float])`  
✅ Friendly: `Number (Int or Float)`

❌ Technical: `IrNode::Apply { ... }`  
✅ Friendly: `This adds 3 numbers together`

### 2. Visual Boxes

All output uses nice Unicode box drawing:
```
┌─ 🔍 TYPE INFORMATION ─────────────────┐
│ ...content...
└────────────────────────────────────────┘
```

### 3. Emoji Indicators

- ✅ Success
- ❌ Error
- ⚠️  Warning
- 🔍 Type info
- 🌳 AST
- 💭 Explanation
- 🔒 Security
- ⏱️  Timing
- 📊 Analysis

### 4. Smart Defaults

- Type checking: **ON** (safety first!)
- Error messages: **Helpful** (not cryptic)
- Auto-features: **OFF** (not overwhelming)
- Help: **Always available** (`:help`)

### 5. Progressive Disclosure

Start simple:
```
rtfs> 42
╭─────────────────────────────────────╮
│ ✅ Result: Integer(42)
╰─────────────────────────────────────╯
```

Add details as needed:
```
rtfs> :type   # Just type info
rtfs> :explain   # What it does
rtfs> :info   # Everything!
```

---

## Common Workflows

### Learning RTFS

```bash
# Step 1: Try simple expressions
rtfs> (+ 1 2)

# Step 2: See types
rtfs> :type

# Step 3: Understand structure
rtfs> :ast

# Step 4: Get full explanation
rtfs> :explain
```

### Developing Features

```bash
# Turn on auto-display
rtfs> :set types on
rtfs> :set timing on

# Now every expression shows types and timing
rtfs> (+ 10 20)
🔍 Type: Number (Int or Float)
╭─────────────────────────────────────╮
│ ✅ Result: Integer(30)
│ ⏱️  Time: 142µs
╰─────────────────────────────────────╯
```

### Security Review

```bash
rtfs> (read-file "data.txt")
rtfs> :security
│ ⚠️  External Operations Detected:
│   • File I/O: read-file
│ 🔐 Security Level: Controlled
```

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Execute (or continue multi-line) |
| `Enter` twice | Execute multi-line code |
| `Ctrl-C` | Cancel multi-line input |
| `Ctrl-D` | Exit REPL |
| `↑/↓` | History navigation |

---

## Troubleshooting

### "No previous expression"

```
rtfs> :type
ℹ️  No previous expression. Try evaluating something first!
```

**Solution**: Type an expression first, then use the command.

### "Parse error"

```
rtfs> (+ 1 2
rtfs>   3)
│ ❌ Parse error: ...
```

**Solution**: Check bracket balancing. Use `reset` to clear buffer.

### "Type Check: FAIL"

```
│ ⚠️  Type warning: Type mismatch...
```

**Solution**: The code will still run, but might have runtime issues. Check types with `:type`.

---

## Comparison with Other REPLs

### Python REPL

```python
>>> type(42)
<class 'int'>
```

### RTFS REPL

```
rtfs> 42
rtfs> :type
│ Expression 1: Integer
```

**Advantage**: More informative, visual, beginner-friendly!

---

## Feature Summary

| Command | What It Does | Best For |
|---------|--------------|----------|
| `:type` | Show type | Learning type system |
| `:ast` | Show syntax tree | Understanding parsing |
| `:ir` | Show internal representation | Advanced debugging |
| `:explain` | Plain language explanation | Beginners |
| `:security` | Security analysis | Safety review |
| `:info` | Everything at once | Comprehensive check |
| `:format` | Prettify code | Code style |
| `:set` | Configure auto-display | Customization |

---

## Advanced: Auto-Display Combinations

```bash
# Show everything after each eval
rtfs> :set types on
rtfs> :set timing on

# Now you get rich feedback automatically
rtfs> (+ 1 2 3)
🔍 Type: Number (Int or Float)
╭─────────────────────────────────────╮
│ ✅ Result: Integer(6)
│ ⏱️  Time: 128µs
╰─────────────────────────────────────╯
```

**Perfect for**: Learning, development, debugging

---

## Theory Behind the Features

All REPL features are based on the formal type system:

- **`:type`** uses the type inference algorithm from §4.2
- **`:security`** scans for capability calls (§7 examples)
- **Type checking** validates against subtyping rules (§3.1)

**See**: [Formal Type System Spec](../specs/13-type-system.md)

---

## FAQ

**Q: Why use `:` for commands?**

A: Easy to type, doesn't conflict with RTFS syntax, discoverable with `:help`.

**Q: Can I disable type checking?**

A: No - REPL always type-checks for safety. Use the compiler with `--no-type-check` if needed.

**Q: What's the difference between `:type` and `:info`?**

A: `:type` shows just type info. `:info` shows type + security + complexity + timing.

**Q: Why are there emojis?**

A: Visual indicators help non-experts quickly identify success/error/info at a glance!

---

**Last Updated**: 2025-11-01  
**See Also**: [Type System Spec](../specs/13-type-system.md), [Type Checking Guide](./type-checking-guide.md)

