# Chat Gateway Setup (Phase 1 MVP)

## Secrets
Secrets are stored locally in:
- .secrets/chat_gateway.env

**Do not commit this file.**

### Regenerate secrets
Run the following commands from the repository root:

1) Create/update the secrets file:

```
umask 077
mkdir -p .secrets
{
  echo "CCOS_QUARANTINE_KEY=$(openssl rand -base64 32)"
  echo "CCOS_CONNECTOR_SECRET=$(openssl rand -base64 48)"
} > .secrets/chat_gateway.env
```

2) Load the secrets into your shell:

```
set -a
source .secrets/chat_gateway.env
set +a
```

## Notes
- `CCOS_QUARANTINE_KEY` must be base64-encoded 32 bytes.
- `CCOS_CONNECTOR_SECRET` should be a strong random value (base64-encoded). 
