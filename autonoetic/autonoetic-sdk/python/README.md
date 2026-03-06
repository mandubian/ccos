# autonoetic_sdk (Python)

Python sandbox SDK for Autonoetic.

```python
import autonoetic_sdk

sdk = autonoetic_sdk.init()
text = sdk.memory.read("task.md")
```

The SDK expects a Unix socket path in `CCOS_SOCKET_PATH` (or explicit `init(socket_path=...)`).
