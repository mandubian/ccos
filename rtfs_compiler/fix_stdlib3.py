#!/usr/bin/env python3

import re
import sys

def fix_stdlib_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()
    
    # Fix all remaining Function::Builtin { ... } patterns (without BuiltinFunction wrapper)
    # This pattern will match any Function::Builtin { with fields inside
    pattern = r'Function::Builtin\s*\{\s*name:\s*"([^"]+)"\.to_string\(\),\s*arity:\s*([^,]+),\s*func:\s*Self::(\w+),\s*\}'
    
    def replacement(match):
        name = match.group(1)
        arity = match.group(2)
        func_name = match.group(3)
        return f'Function::Builtin(BuiltinFunction {{\n            name: "{name}".to_string(),\n            arity: {arity},\n            func: Rc::new(Self::{func_name}),\n        }})'
    
    content = re.sub(pattern, replacement, content, flags=re.MULTILINE | re.DOTALL)
    
    # Also fix the pattern in reduce function
    content = content.replace('Function::Builtin { func, .. }', 'Function::Builtin(builtin_func)')
    content = content.replace('func.clone()', 'builtin_func.func.clone()')
    
    with open(filepath, 'w') as f:
        f.write(content)

if __name__ == "__main__":
    fix_stdlib_file("/home/mandubian/workspaces/mandubian/rtfs-ai/rtfs_compiler/src/runtime/stdlib.rs")
    print("Fixed remaining Function::Builtin patterns")
