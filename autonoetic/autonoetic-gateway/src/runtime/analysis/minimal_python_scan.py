#!/usr/bin/env python3
"""Minimal static scan for Autonoetic agent.install (stdlib only).

Reads JSON from stdin:
  {"files": [{"path": "str", "content": "str"}, ...]}

Writes JSON to stdout:
  inferred_types, evidence, threats, remote_access_detected

Only *.py paths are parsed; other files are skipped.
"""
from __future__ import annotations

import ast
import json
import sys
from typing import Optional


def _call_name(node: ast.AST) -> str:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        base = _call_name(node.value)
        return f"{base}.{node.attr}" if base else node.attr
    return ""


class ScanVisitor(ast.NodeVisitor):
    def __init__(self, path: str) -> None:
        self.path = path
        self.inferred = set()
        self.evidence = []
        self.threats = []
        self.remote = False

    def add_cap(self, cap: str, line: int, pattern: str) -> None:
        self.inferred.add(cap)
        self.evidence.append(
            {
                "file": self.path,
                "line": line,
                "pattern": pattern,
                "capability_type": cap,
                "confidence": 0.88,
            }
        )
        if cap == "NetworkAccess":
            self.remote = True

    def visit_Import(self, node: ast.Import) -> None:
        for alias in node.names:
            base = alias.name.split(".")[0]
            if base in _NET_BASES:
                self.add_cap("NetworkAccess", node.lineno, f"import {alias.name}")
        self.generic_visit(node)

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        if node.module:
            base = node.module.split(".")[0]
            if base in _NET_BASES:
                self.add_cap("NetworkAccess", node.lineno, f"from {node.module}")
        self.generic_visit(node)

    def visit_Call(self, node: ast.Call) -> None:
        ln = getattr(node, "lineno", 0) or 0
        fn = _call_name(node.func)

        if isinstance(node.func, ast.Name) and node.func.id == "open":
            if len(node.args) >= 2:
                arg1 = node.args[1]
                if isinstance(arg1, ast.Constant) and isinstance(arg1.value, str):
                    if any(c in arg1.value for c in "wax+") or "a" in arg1.value:
                        self.add_cap("WriteAccess", ln, "open(..., write mode)")
            for kw in node.keywords or []:
                if kw.arg == "mode" and isinstance(kw.value, ast.Constant):
                    v = kw.value.value
                    if isinstance(v, str) and any(c in v for c in "wax+"):
                        self.add_cap("WriteAccess", ln, "open(..., mode=write)")

        if fn in ("os.remove", "os.unlink", "os.rmdir", "pathlib.Path.unlink"):
            self.add_cap("WriteAccess", ln, fn)
            self._threat("destructive", "medium", f"file deletion: {fn}", ln, fn)
        if fn == "shutil.rmtree":
            self.add_cap("WriteAccess", ln, fn)
            self._threat("destructive", "high", f"recursive delete: {fn}", ln, fn)

        if fn and fn.startswith("subprocess."):
            self.add_cap("CodeExecution", ln, fn)
            shell = False
            for kw in node.keywords or []:
                if kw.arg == "shell" and isinstance(kw.value, ast.Constant):
                    shell = bool(kw.value.value)
            if shell:
                self._threat(
                    "command_injection",
                    "critical",
                    f"{fn} with shell=True",
                    ln,
                    fn,
                )
            else:
                self._threat(
                    "remote_code_execution",
                    "high",
                    f"subprocess invoke: {fn}",
                    ln,
                    fn,
                )

        if fn == "os.system":
            self.add_cap("CodeExecution", ln, fn)
            self._threat("command_injection", "high", "os.system", ln, "os.system")

        if isinstance(node.func, ast.Name) and node.func.id in ("eval", "exec"):
            self._threat("command_injection", "critical", node.func.id, ln, node.func.id)

        self.generic_visit(node)

    def _threat(
        self,
        ttype: str,
        sev: str,
        desc: str,
        line: int,
        pat: str,
    ) -> None:
        self.threats.append(
            {
                "threat_type": ttype,
                "severity": sev,
                "description": desc,
                "file": self.path,
                "line": line,
                "pattern": pat,
                "confidence": 0.88,
            }
        )


_NET_BASES = frozenset(
    {
        "urllib",
        "urllib3",
        "http",
        "socket",
        "requests",
        "httpx",
        "aiohttp",
        "ftplib",
        "smtplib",
        "ssl",
        "websockets",
        "websocket",
    }
)


def scan_file(path: str, content: str) -> Optional[ScanVisitor]:
    if not path.endswith(".py"):
        return None
    try:
        tree = ast.parse(content, filename=path)
    except SyntaxError:
        return None
    v = ScanVisitor(path)
    v.visit(tree)
    return v


def main() -> None:
    data = json.load(sys.stdin)
    out_inf = set()
    out_ev = []
    out_th = []
    remote = False
    for f in data.get("files", []):
        path = f.get("path") or ""
        content = f.get("content") or ""
        v = scan_file(path, content)
        if not v:
            continue
        out_inf |= v.inferred
        out_ev.extend(v.evidence)
        out_th.extend(v.threats)
        remote = remote or v.remote
    print(
        json.dumps(
            {
                "inferred_types": sorted(out_inf),
                "evidence": out_ev,
                "threats": out_th,
                "remote_access_detected": remote,
            }
        )
    )


if __name__ == "__main__":
    main()
