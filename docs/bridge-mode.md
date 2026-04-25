# Bridge Mode Operator Guide

**Feature flag:** `bridge`  
**Last Updated:** 2026-04-25  
**Related:** `docs/operations.md`, `crates/oxicode-cli/src/remote/`

## Overview

Bridge mode launches OxiCode as a headless WebSocket server for IDE integration and cloud deployment. Each connected client gets an isolated `QueryEngine` instance. Authentication is JWT (HS256) with a shared secret.

## Starting the Server

```bash
# Build with bridge feature
cargo build --features bridge

# Start on default port 8080
OXICODE_BRIDGE_JWT_SECRET=my-secret oxicode --bridge

# Custom port
OXICODE_BRIDGE_JWT_SECRET=my-secret oxicode --bridge --port 9090
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `OXICODE_BRIDGE_JWT_SECRET` | *(required)* | HS256 signing secret for JWT validation |
| `OXICODE_BRIDGE_MAX_SESSIONS` | `16` | Max concurrent WebSocket sessions |
| `OXICODE_BRIDGE_IDLE_TIMEOUT_SECS` | `600` | Seconds before idle session is evicted |
| `OXICODE_BRIDGE_TLS_CERT` | *(none)* | Path to PEM certificate (TLS not yet implemented — see warning below) |
| `OXICODE_BRIDGE_TLS_KEY` | *(none)* | Path to PEM private key (TLS not yet implemented) |

## WebSocket Subprotocol

Clients must request the subprotocol `oxicode.bridge.v1` in the handshake header:

```
Sec-WebSocket-Protocol: oxicode.bridge.v1
```

Connections that do not negotiate this subprotocol are rejected with close code `4400`.

## JWT Authentication

The first message from the client must carry a valid JWT in the `Authorization` header of the HTTP upgrade request, or as the `token` field of the first JSON message.

**Required claims:**

| Claim | Description |
|---|---|
| `sub` | Session subject / user identifier |
| `exp` | Expiration timestamp (Unix seconds) |
| `iss` | Issuer (validated if `OXICODE_BRIDGE_JWT_ISSUER` is set) |

**Issuing a token (`jsonwebtoken` crate, HS256):**

```rust
use jsonwebtoken::{encode, Header, EncodingKey};
#[derive(serde::Serialize)]
struct Claims { sub: String, exp: usize }

let claims = Claims {
    sub: "user@example.com".into(),
    exp: (std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() + 3600) as usize,
};
let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(b"my-secret"))?;
```

Invalid or expired tokens are rejected with close code `4401`.

## Message Types

All messages are JSON objects sent over the WebSocket text channel.

**Inbound (client → server):**

| `type` | Description |
|---|---|
| `user_message` | User turn; field `content: String` |
| `cancel` | Abort the current in-flight turn |
| `slash_command` | Execute a slash command; field `command: String` |
| `permission_response` | Reply to a `permission_ask`; field `allow: bool` |

**Outbound (server → client):**

| `type` | Description |
|---|---|
| `session_start` | Session accepted; fields `session_id`, `model` |
| `text_delta` | Streaming text chunk; field `text: String` |
| `tool_use` | Tool invocation started; fields `id`, `name`, `input` |
| `tool_result` | Tool completed; fields `id`, `output`, `is_error: bool` |
| `permission_ask` | Permission needed; fields `tool`, `description` |
| `usage` | Token counts; fields `input_tokens`, `output_tokens` |
| `error` | Non-fatal error; field `message: String` |
| `session_end` | Session closing; field `reason: String` |

## Session Limits

- Max concurrent sessions: `OXICODE_BRIDGE_MAX_SESSIONS` (default 16). New connections are rejected with close code `4429` when the cap is reached.
- Idle timeout: sessions inactive for `OXICODE_BRIDGE_IDLE_TIMEOUT_SECS` receive a `session_end` message then are closed.
- Per-session tool-turn limit: 50 (same as TUI path, enforced by `QueryEngine`).

## TLS Warning

> **TLS is not yet implemented.** `OXICODE_BRIDGE_TLS_CERT` and `OXICODE_BRIDGE_TLS_KEY` are defined but ignored. A follow-up phase will wire `tokio-rustls`. Until then, use a TLS-terminating reverse proxy for any non-localhost deployment.

## Graceful Shutdown

On `SIGINT` or `SIGTERM`, the server stops accepting new connections, sends a `session_end` message to all active sessions, and exits cleanly after draining.

## Security Recommendations

- Use a randomly-generated secret of at least 32 bytes for `OXICODE_BRIDGE_JWT_SECRET`. Never log it.
- Rotate secrets after any suspected compromise.
- Set short JWT `exp` values (1–24 hours); re-issue via your auth service.
- For non-localhost deployments, use a TLS-terminating proxy (nginx, Caddy, AWS ALB) until native TLS ships.
