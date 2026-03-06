"""Autonoetic sandbox SDK client."""

from __future__ import annotations

import json
import os
import socket
from dataclasses import dataclass
from typing import Any

from . import errors

CCOS_SOCKET_ENV = "CCOS_SOCKET_PATH"


def _require_socket_path(path: str | None) -> str:
    if path is not None and path.strip():
        return path
    env = os.getenv(CCOS_SOCKET_ENV)
    if env and env.strip():
        return env
    raise RuntimeError(f"Missing required socket path: pass init(socket_path=...) or set {CCOS_SOCKET_ENV}")


@dataclass
class _RpcClient:
    socket_path: str
    _next_id: int = 1

    def request(self, method: str, params: dict[str, Any]) -> Any:
        payload = {
            "jsonrpc": "2.0",
            "id": self._next_id,
            "method": method,
            "params": params,
        }
        self._next_id += 1

        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
            conn.connect(self.socket_path)
            conn.sendall(json.dumps(payload).encode("utf-8") + b"\n")
            data = b""
            while not data.endswith(b"\n"):
                chunk = conn.recv(4096)
                if not chunk:
                    raise RuntimeError(f"Gateway closed socket before response for method {method}")
                data += chunk

        response = json.loads(data.decode("utf-8").strip())
        if "error" in response and response["error"] is not None:
            self._raise_mapped_error(response["error"])
        if "result" not in response:
            raise RuntimeError(f"Invalid JSON-RPC response (missing result) for method {method}")
        return response["result"]

    @staticmethod
    def _raise_mapped_error(err: dict[str, Any]) -> None:
        message = str(err.get("message", "Unknown gateway error"))
        data = err.get("data") or {}
        err_type = str(data.get("error_type", "")).lower()
        if err_type == "policy_violation":
            raise errors.PolicyViolation(message)
        if err_type == "rate_limit_exceeded":
            raise errors.RateLimitExceeded(message)
        if err_type == "approval_required":
            secret = str(data.get("secret_name", ""))
            raise errors.ApprovalRequiredError(secret, message)
        raise errors.AutonoeticSdkError(message)


class _MemoryApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def read(self, path: str) -> str:
        result = self._rpc.request("memory.read", {"path": path})
        return str(result["content"])

    def write(self, path: str, content: bytes | str) -> Any:
        if isinstance(content, bytes):
            content = content.decode("utf-8")
        return self._rpc.request("memory.write", {"path": path, "content": content})

    def list_keys(self) -> list[str]:
        result = self._rpc.request("memory.list_keys", {})
        return list(result["keys"])

    def remember(self, key: str, value: Any) -> Any:
        return self._rpc.request("memory.remember", {"key": key, "value": value})

    def recall(self, key: str) -> Any:
        result = self._rpc.request("memory.recall", {"key": key})
        return result.get("value")

    def search(self, query: str) -> list[str]:
        result = self._rpc.request("memory.search", {"query": query})
        return list(result.get("results", []))


class _StateApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def checkpoint(self, data: Any) -> Any:
        return self._rpc.request("state.checkpoint", {"data": data})

    def get_checkpoint(self) -> Any:
        result = self._rpc.request("state.get_checkpoint", {})
        return result.get("data")


class _SecretsApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def get(self, name: str) -> str:
        result = self._rpc.request("secrets.get", {"name": name})
        return str(result["value"])


class _MessageApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def send(self, agent_id: str, payload: Any) -> Any:
        return self._rpc.request("message.send", {"agent_id": agent_id, "payload": payload})

    def ask(self, agent_id: str, question: str) -> Any:
        result = self._rpc.request("message.ask", {"agent_id": agent_id, "question": question})
        return result.get("answer")


class _FilesApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def download(self, url: str) -> Any:
        return self._rpc.request("files.download", {"url": url})

    def upload(self, path: str, target: str) -> Any:
        return self._rpc.request("files.upload", {"path": path, "target": target})


class _ArtifactsApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def put(self, path: str, visibility: str = "private") -> Any:
        return self._rpc.request("artifacts.put", {"path": path, "visibility": visibility})

    def mount(self, ref: str, target_path: str) -> Any:
        return self._rpc.request("artifacts.mount", {"ref": ref, "target_path": target_path})

    def share(self, ref: str, agent_id: str) -> Any:
        return self._rpc.request("artifacts.share", {"ref": ref, "agent_id": agent_id})


class _EventsApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def emit(self, type: str, data: Any) -> Any:
        return self._rpc.request("events.emit", {"type": type, "data": data})


class _TasksApi:
    def __init__(self, rpc: _RpcClient) -> None:
        self._rpc = rpc

    def post(self, title: str, description: str, assignee: str | None = None) -> Any:
        return self._rpc.request(
            "tasks.post",
            {"title": title, "description": description, "assignee": assignee},
        )

    def claim(self) -> Any:
        result = self._rpc.request("tasks.claim", {})
        return result.get("task")

    def complete(self, task_id: str, result: Any) -> Any:
        return self._rpc.request("tasks.complete", {"task_id": task_id, "result": result})

    def list(self, status: str | None = None) -> list[Any]:
        result = self._rpc.request("tasks.list", {"status": status})
        return list(result.get("tasks", []))


class AutonoeticSdk:
    """Top-level SDK surface for sandbox scripts."""

    def __init__(self, socket_path: str) -> None:
        rpc = _RpcClient(socket_path)
        self.memory = _MemoryApi(rpc)
        self.state = _StateApi(rpc)
        self.secrets = _SecretsApi(rpc)
        self.message = _MessageApi(rpc)
        self.files = _FilesApi(rpc)
        self.artifacts = _ArtifactsApi(rpc)
        self.events = _EventsApi(rpc)
        self.tasks = _TasksApi(rpc)


def init(socket_path: str | None = None) -> AutonoeticSdk:
    """Initialize the SDK from an explicit path or CCOS_SOCKET_PATH."""
    return AutonoeticSdk(_require_socket_path(socket_path))
