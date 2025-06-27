#!/usr/bin/env python3

import re
import sys

def fix_stdlib_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()
    
    # Fix function definitions to use correct Function::Builtin structure
    content = re.sub(
        r'Value::Function\(Function::Builtin \{\s*name: "([^"]+)"\.to_string\(\),\s*arity: Arity::(\w+)\((\d+)\),\s*func: Self::(\w+),\s*\}\)',
        r'Value::Function(Function::Builtin(BuiltinFunction {\n            name: "\1".to_string(),\n            arity: Arity::\2(\3),\n            func: Rc::new(Self::\4),\n        }))',
        content,
        flags=re.MULTILINE | re.DOTALL
    )
    
    # Fix Arity enum variants
    content = content.replace('Arity::AtLeast', 'Arity::Variadic')
    content = content.replace('Arity::Exact', 'Arity::Fixed') 
    content = content.replace('Arity::Any', 'Arity::Variadic(0)')
    
    # Fix function signatures to accept Vec<Value> instead of &[Value]
    content = re.sub(
        r'fn (\w+)\(args: &\[Value\]\) -> RuntimeResult<Value> \{',
        r'fn \1(args: Vec<Value>) -> RuntimeResult<Value> {\n        let args = args.as_slice();',
        content
    )
    
    with open(filepath, 'w') as f:
        f.write(content)

if __name__ == "__main__":
    fix_stdlib_file("/home/mandubian/workspaces/mandubian/rtfs-ai/rtfs_compiler/src/runtime/stdlib.rs")
    print("Fixed stdlib.rs")
