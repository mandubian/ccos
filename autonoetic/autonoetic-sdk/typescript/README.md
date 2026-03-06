# autonoetic_sdk (TypeScript)

TypeScript sandbox SDK for Autonoetic.

```ts
import { init } from "autonoetic_sdk";

const sdk = init();
const text = await sdk.memory.read("task.md");
```

The SDK expects `CCOS_SOCKET_PATH` (or explicit `init({ socketPath })`).
