# Operations Guide

## OTLP Telemetry

OxiCode can export traces to any OpenTelemetry-compatible backend (Jaeger, Grafana Tempo, Datadog, etc.) via the `telemetry-otlp` feature flag.

### Enable

Build or run with the feature flag:

```bash
cargo build --features telemetry-otlp
cargo run --features telemetry-otlp
```

### Configuration

All settings are read from environment variables at startup. Missing or malformed values fall back to defaults — startup is never blocked.

| Variable | Default | Description |
|---|---|---|
| `OXICODE_OTLP_ENDPOINT` | `http://localhost:4317` | gRPC OTLP endpoint |
| `OXICODE_OTLP_HEADERS` | *(none)* | Auth headers: `key=value,key=value` |
| `OXICODE_OTLP_SAMPLE_RATE` | `0.1` | Trace sample rate `[0.0, 1.0]` |

**Security:** `OXICODE_OTLP_HEADERS` values are never written to logs.

### Quick Start with Jaeger

```bash
docker run -d --name jaeger \
  -p 16686:16686 \
  -p 4317:4317 \
  jaegertracing/all-in-one:latest

OXICODE_OTLP_SAMPLE_RATE=1.0 cargo run --features telemetry-otlp
```

Open `http://localhost:16686` and select the `oxicode-cli` service.

### Authenticated Backend (e.g. Grafana Cloud)

```bash
export OXICODE_OTLP_ENDPOINT=https://tempo-prod.grafana.net:443
export OXICODE_OTLP_HEADERS="Authorization=Bearer <token>"
export OXICODE_OTLP_SAMPLE_RATE=0.05
cargo run --features telemetry-otlp
```

### Default Build

Without `--features telemetry-otlp`, no OTLP dependencies are compiled in. Tracing writes to the local log file only (`~/.local/share/oxicode/oxicode.log` on Linux/macOS).

---

## Bridge Mode

Headless WebSocket server for IDE integration and cloud deployment. See the full operator guide: [bridge-mode.md](./bridge-mode.md).

**Quick start:**

```bash
cargo build --features bridge
OXICODE_BRIDGE_JWT_SECRET=my-secret oxicode --bridge --port 8080
```

Key env vars: `OXICODE_BRIDGE_JWT_SECRET` (required), `OXICODE_BRIDGE_MAX_SESSIONS` (default 16), `OXICODE_BRIDGE_IDLE_TIMEOUT_SECS` (default 600).
