#!/usr/bin/env python3

import re
import sys

def fix_stdlib_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()
    
    # Fix all Function::Builtin definitions that don't have the correct structure
    # Pattern 1: Function::Builtin { name: ..., arity: ..., func: ... }
    pattern1 = r'Function::Builtin \{\s*name: "([^"]+)"\.to_string\(\),\s*arity: Arity::(\w+)(\([^)]*\))?,\s*func: Self::(\w+),\s*\}'
    replacement1 = r'Function::Builtin(BuiltinFunction {\n            name: "\1".to_string(),\n            arity: Arity::\2\3,\n            func: Rc::new(Self::\4),\n        })'
    content = re.sub(pattern1, replacement1, content, flags=re.MULTILINE | re.DOTALL)
    
    # Also handle cases where there might be extra spaces or variations
    pattern2 = r'Function::Builtin\s*\{\s*name:\s*"([^"]+)"\.to_string\(\),\s*arity:\s*Arity::(\w+)(\([^)]*\))?,\s*func:\s*Self::(\w+),\s*\}'
    replacement2 = r'Function::Builtin(BuiltinFunction {\n            name: "\1".to_string(),\n            arity: Arity::\2\3,\n            func: Rc::new(Self::\4),\n        })'
    content = re.sub(pattern2, replacement2, content, flags=re.MULTILINE | re.DOTALL)
    
    with open(filepath, 'w') as f:
        f.write(content)

if __name__ == "__main__":
    fix_stdlib_file("/home/mandubian/workspaces/mandubian/rtfs-ai/rtfs_compiler/src/runtime/stdlib.rs")
    print("Fixed remaining stdlib.rs patterns")
