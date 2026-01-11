# RTFS-CCOS-REPL Guide

Interactive REPL for RTFS language with full CCOS capability integration.

## Quick Start

```bash
# Run the REPL
cargo run --bin rtfs-ccos-repl

# Execute a single expression and exit
cargo run --bin rtfs-ccos-repl -- -e "(+ 1 2 3)"

# Execute a file then enter REPL
cargo run --bin rtfs-ccos-repl script.rtfs

# Enable verbose output
cargo run --bin rtfs-ccos-repl -v -e "(+ 1 2)"
```

## Command-Line Options

| Option | Description |
|--------|-------------|
| `[FILE]` | RTFS file to execute |
| `-e, --expr <EXPR>` | Execute expression and exit |
| `-s, --security <level>` | Security level: minimal, standard, paranoid, custom |
| `--enable <category>` | Enable capability category (system, fileio, network, agent, ai, data, logging) |
| `--disable <category>` | Disable capability category |
| `--timeout <ms>` | Maximum execution time (default: 30000ms) |
| `-v, --verbose` | Enable verbose output |
| `--allow <capability>` | Allow specific capability |
| `--deny <capability>` | Deny specific capability |
| `--http-real` | Use real HTTP provider (not mock) |
| `--http-allow <host>` | Allow outbound HTTP hostnames |
| `--microvm-provider <provider>` | Select MicroVM provider (mock, process) |

## Interactive REPL Commands

| Command | Description |
|---------|-------------|
| `help` or `:h` | Show help |
| `stats` or `:stats` | Show environment statistics |
| `caps` or `:caps` | List available capabilities |
| `config` or `:config` | Interactive configuration menu |
| `clear` or `:clear` | Clear screen |
| `:load <file>` | Load and execute RTFS file |
| `quit`, `exit`, `:q` | Exit REPL |

## Security Levels

| Level | Description |
|-------|-------------|
| `minimal` | Basic security, most capabilities allowed |
| `standard` | Balanced security and functionality |
| `paranoid` | Maximum security, restricted capabilities |

## Capability Categories

| Category | Icon | Description |
|----------|------|-------------|
| System | 🖥️ | Environment, time, process operations |
| FileIO | 📁 | File reading, writing, directory access |
| Network | 🌐 | HTTP requests, network communication |
| Agent | 🤖 | Inter-agent communication, discovery |
| AI | 🧠 | LLM inference, AI model operations |
| Data | 📊 | JSON parsing, data manipulation |
| Logging | 📝 | Output logging, debugging info |

## RTFS Syntax Examples

### Basic Arithmetic

```lisp
(+ 1 2 3)                    ; => 6
(- 10 5 2)                   ; => 3
(* 3 4 5)                    ; => 60
(/ 20 4 2)                   ; => 2.5
```

### Variables and Let Binding

```lisp
(let [x 10 y 20] (+ x y))    ; => 30
(let [x 5] (* x x))          ; => 25
```

### Conditionals

```lisp
(if (> 10 5) "yes" "no")     ; => "yes"
(if (< 3 1) "a" "b")         ; => "b"
```

### String Operations

```lisp
(str "hello" " " "world")    ; => "hello world"
(println "Hello!")           ; prints "Hello!"
```

### Lists and Collections

```lisp
(first [1 2 3 4 5])          ; => 1
(rest [1 2 3 4 5])           ; => [2 3 4 5]
(nth [10 20 30 40] 2)        ; => 30
(range 0 5)                  ; => [0 1 2 3 4]
```

### Map, Filter, Reduce

```lisp
(map inc [1 2 3 4 5])        ; => [2 3 4 5 6]
(filter even? [1 2 3 4 5])   ; => [2 4]
(reduce + 0 [1 2 3 4 5])     ; => 15
```

### Higher-Order Functions

```lisp
(map (fn [x] (* x x)) (range 1 5))  ; => [1 4 9 16]
(filter odd? [1 2 3 4 5 6])         ; => [1 3 5]
```

### Do Blocks

```lisp
(do
  (let [x 5] (* x x))        ; => 25
  (let [y 3] (* y y)))       ; => 9 (returns last value)
```

### Merging Maps

```lisp
(merge {:a 1} {:b 2})        ; => {:a 1, :b 2}
```

### Predicates

```lisp
(contains? [1 2 3 4 5] 3)    ; => true
(even? 4)                    ; => true
(odd? 5)                     ; => true
(distinct [1 2 2 3 3 3])     ; => [1 2 3]
```

### Sorting

```lisp
(sort [5 2 8 1 9 3])         ; => [1 2 3 5 8 9]
```

## Advanced Usage

### Using Real HTTP Provider

```bash
cargo run --bin rtfs-ccos-repl -- --http-real --http-allow api.example.com
```

### Enabling Specific Capabilities

```bash
cargo run --bin rtfs-ccos-repl -- --enable network --enable fileio
```

### Custom Security Level

```bash
cargo run --bin rtfs-ccos-repl -- --security paranoid
```

### Running with Timeout

```bash
cargo run --bin rtfs-ccos-repl -- --timeout 5000 -e "(while true 1)"
```

## Configuration Menu

The interactive REPL includes a configuration menu (accessed via `config` command):

1. **Security Level** - Change security settings
2. **Capabilities** - Enable/disable capability categories
3. **Current Config** - View active configuration
4. **Back** - Return to main REPL

## Examples

### Fibonacci Sequence

```lisp
(let [fib (fn [n]
            (if (< n 2)
              n
              (+ (fib (- n 1)) (fib (- n 2)))))]
  (map fib (range 0 10)))
```

### Filtering and Mapping Combined

```lisp
(filter odd?
  (map (fn [x] (* x x))
    (range 1 10)))
```

### Conditional Processing

```lisp
(map (fn [x]
       (if (even? x)
         (str x " is even")
         (str x " is odd")))
  (range 1 6))
```

## Troubleshooting

### Unknown Capability

If you get `UnknownCapability("ccos.xxx")`, the capability may be disabled. Enable it with:

```bash
cargo run --bin rtfs-ccos-repl -- --enable <category>
```

### Execution Timeout

Increase the timeout:

```bash
cargo run --bin rtfs-ccos-repl -- --timeout 60000
```

### Arity Mismatch

Functions like `range` require multiple arguments:

```lisp
(range 5)      ; Error: expects 2 arguments
(range 0 5)    ; Works: returns [0 1 2 3 4]
```

## Related Documentation

- [RTFS Language Overview](../../rtfs-2.0/specs/01-language-overview.md)
- [CCOS Architecture](../specs/000-ccos-architecture.md)
- [Capability Marketplace](../specs/004-capabilities-and-marketplace.md)
