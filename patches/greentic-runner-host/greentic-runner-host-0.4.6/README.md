# greentic-runner-host

`greentic-runner-host` packages the Greentic runner as a standalone crate. It owns tenant bindings, the pack watcher, Wasmtime glue, canonical ingress adapters (Telegram, Teams, Slack, WebChat, Webex, WhatsApp, generic webhook, timers), the state machine (pause/resume, session/state persistence), and admin/health endpoints. Binaries such as `greentic-runner` or `greentic-demo` embed this crate instead of vendoring runtime internals.

## Architecture highlights

- **Pack ingestion** – consumes a JSON index (local path, HTTPS, or cloud bucket) via `runner-core`, verifies signatures/digests (`PACK_PUBLIC_KEY`, `PACK_VERIFY_STRICT`), caches artifacts under `PACK_CACHE_DIR`, and supports ordered overlays per tenant.
- **Hot reload** – `PACK_REFRESH_INTERVAL` drives a watcher that resolves the index, preloads packs, and swaps tenant runtimes atomically; `/admin/packs/reload` triggers the same path on demand. Overlays can be added/removed without touching the base pack.
- **Canonical ingress** – all adapters normalize raw provider payloads into the shared schema:
  ```json
  {
    "tenant": "demo",
    "provider": "slack",
    "provider_ids": {
      "workspace_id": "T123",
      "channel_id": "C789",
      "thread_id": "1731315600.000100",
      "user_id": "U456",
      "message_id": "1731315600.000100",
      "event_id": "Ev01ABC"
    },
    "session": {
      "key": "demo:slack:1731315600.000100:U456",
      "scopes": ["chat","attachments","buttons"]
    },
    "timestamp": "2025-11-11T09:00:00Z",
    "text": "Hi",
    "attachments": [],
    "buttons": [],
    "entities": { "mentions": [], "urls": [] },
    "metadata": { "raw_headers": {}, "ip": null },
    "channel_data": { "type": "message" },
    "raw": { "...": "original provider payload" }
  }
  ```
  Canonical session keys follow `{tenant}:{provider}:{conversation-or-thread-or-channel}:{user}`, ensuring pause/resume and dedupe behave consistently per adapter.
- **Sessions & state** – the host bundles `greentic-session`/`greentic-state`. Multi-turn flows pause via `session.wait`; the runtime stores `FlowSnapshot`s keyed by the canonical session, resumes on the next ingress event, and clears the entry on completion. Packs automatically receive the `state.get/state.set/session.update` host interface (WIT v0.6).
- **Telemetry & admin** – optional OTLP bootstrapping (`greentic-telemetry`), `/healthz`, and bearer-protected `/admin` endpoints (loopback-only when `ADMIN_TOKEN` is unset).

### Pack index format

Pack ingestion is driven by a JSON index (see `examples/index.json`). Each tenant entry declares a `main_pack` plus optional `overlays`, letting you layer overrides without rebuilding the base artifact:

```json
{
  "tenants": {
    "demo": {
      "main_pack": {
        "reference": { "name": "demo-pack", "version": "1.2.3" },
        "locator": "fs:///packs/demo.gtpack",
        "digest": "sha256:abcd...",
        "signature": "ed25519:...",
        "path": "./packs/demo.gtpack"
      },
      "overlays": [
        {
          "reference": { "name": "demo-overlay", "version": "1.2.3" },
          "path": "./packs/demo_overlay.gtpack",
          "digest": "sha256:efgh..."
        }
      ]
    }
  }
}
```

During a reload the watcher resolves each locator (filesystem, HTTPS, OCI, S3, GCS, Azure blob), verifies digests/signatures, caches artifacts, and constructs a `TenantRuntime` that loads the main pack plus overlays in order. Overlay changes are safe to deploy independently—`crates/tests/tests/host_integration.rs` includes regression coverage.

### Pause & resume semantics

Packs can pause mid-flow by emitting the `session.wait` component. The host persists the `FlowSnapshot` (current node pointer + execution state) into `greentic-session`. The next inbound activity for the same canonical session key (`tenant:provider:channel:conversation:user`) automatically resumes the stored snapshot, continues execution, and clears the entry when the flow completes. This makes multi-message LLM flows and human-in-the-loop approvals idempotent without bespoke session wiring.

### OAuth broker integration

Each tenant bindings file may optionally declare an `oauth` block:

```yaml
oauth:
  http_base_url: https://oauth.api.greentic.net/
  nats_url: nats://oauth-broker:4222
  provider: greentic.oauth.default
  env: prod        # optional, falls back to GREENTIC_ENV/local
  team: platform   # optional logical scope
```

When present, the host wires `greentic-oauth-host`, keeps a tenant-scoped
client configuration, and registers the `greentic:oauth-broker@1.0.0/world
broker` WIT world in Wasmtime. Packs that import the world can ask the host for
consent URLs, exchange codes for tokens, or fetch stored tokens, while
environments without the block behave exactly as before (no broker world is
wired).

## Quick start

```rust
use greentic_runner_host::{Activity, HostBuilder, HostConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = HostConfig::load_from_path("./bindings/customera.yaml")?;
    let host = HostBuilder::new().with_config(config).build()?;

    host.start().await?;
    host.load_pack("customera", "./packs/customera/index.ygtc".as_ref()).await?;

    let activity = Activity::text("hello")
        .with_tenant("customera")
        .from_user("u-1");

    let replies = host.handle_activity("customera", activity).await?;
    for reply in replies {
        println!("reply: {}", reply.payload());
    }

    host.stop().await?;
    Ok(())
}
```

## Cargo features

- `verify` *(default)* – validate pack files exist before loading.
- `mcp` – enable tool invocation through the [`mcp-exec`](https://crates.io/crates/mcp-exec) bridge.
- `telemetry` – wire OTLP export via [`greentic-telemetry`](https://crates.io/crates/greentic-telemetry).

## Environment

| Variable | Description | Default |
| --- | --- | --- |
| `PACK_SOURCE` | Default resolver scheme when the index omits one (`fs`, `http`, `oci`, `s3`, `gcs`, `azblob`) | `fs` |
| `PACK_INDEX_URL` | URI or path to the pack index consumed by the watcher | _required_ |
| `PACK_CACHE_DIR` | Location for the content-addressed pack cache | `.packs` |
| `PACK_PUBLIC_KEY` | Optional Ed25519 key (`ed25519:BASE64...`) to enforce signature checks | _unset_ |
| `PACK_VERIFY_STRICT` | Force signature verification even when no `PACK_PUBLIC_KEY` is provided (`1/true`), or opt out when the key is present (`0/false`) | `PACK_PUBLIC_KEY` driven |
| `PACK_REFRESH_INTERVAL` | Interval used by the background watcher (`30s`, `5m`, etc.) | `30s` |
| `TENANT_RESOLVER` | Router mode for HTTP requests (`host`, `header`, `jwt`, `env`) | `env` |
| `DEFAULT_TENANT` | Fallback tenant identifier when the resolver cannot infer one | `demo` |
| `SECRETS_BACKEND` | Secrets provider to initialise (`env`, `aws`, `gcp`, `azure`) | `env` |
| `OTEL_SERVICE_NAME` | Overrides the OTLP service name advertised to the collector | `greentic-runner-host` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Explicit OTLP collector endpoint | provider preset / unset |
| `ADMIN_TOKEN` | Bearer token required for `/admin` endpoints (loopback-only access if unset) | _unset_ |
| `SLACK_SIGNING_SECRET` | HMAC secret for Slack Events/Interactive adapters | _unset_ |
| `WEBEX_WEBHOOK_SECRET` | Signature key for Cisco Webex webhook validation | _unset_ |
| `WHATSAPP_VERIFY_TOKEN` / `WHATSAPP_APP_SECRET` | Verification + signature secrets for WhatsApp Cloud API | _unset_ |
| `PACK_VERIFY_STRICT` | Enforce signature checks even without a public key | driven by key |

## Admin API

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/healthz` | Liveness check (telemetry, secrets, active packs) |
| `GET` | `/admin/packs/status` | Lists loaded tenants, versions, and digests plus last reload info |
| `POST` | `/admin/packs/reload` | Triggers an immediate pack refresh via the watcher |

If `ADMIN_TOKEN` is set, clients must send `Authorization: Bearer <token>`; otherwise, admin endpoints are limited to loopback connections.

## Ingress adapters

| Adapter | Route | Session anchor | Notes / Env |
| --- | --- | --- | --- |
| Telegram Bot API | `POST /messaging/telegram/webhook` | `chat.id:user.id` (fallback to `user.id`) | Uses `update_id` for dedupe; outbound path relies on `TELEGRAM_BOT_TOKEN` |
| Microsoft Teams (Bot Framework) | `POST /teams/activities` | `replyToId` → `conversation.id` → channel | Accepts Activities JSON (`channelData`, attachments) |
| Slack Events API | `POST /slack/events` | `thread_ts` → `channel` | Requires `SLACK_SIGNING_SECRET`, dedupes via `event_id`, handles retries |
| Slack Interactive | `POST /slack/interactive` | `channel`/`thread` from payload | Same signing secret; parses `payload=` form body |
| WebChat / Direct Line | `POST /webchat/activities` | `conversation.id` | Mirrors Bot Framework schema; attachments mapped 1:1 |
| Cisco Webex | `POST /webex/webhook` | `parentId` → `roomId` | Optional `WEBEX_WEBHOOK_SECRET`; keeps `requires_auth` metadata for file URLs |
| WhatsApp Cloud API | `GET/POST /whatsapp/webhook` | `messages[].from` | `WHATSAPP_VERIFY_TOKEN` (challenge) + `WHATSAPP_APP_SECRET` (signature); interactive/list replies → canonical buttons |
| Generic Webhook | `ANY /webhook/:flow_id` | `Idempotency-Key` header (if present) | Wraps method/path/headers/body into canonical payload |
| Timers / Cron | Defined in `bindings.yaml` | `schedule_id` | Schedules flows with normalized cron (seconds field injected) |

Each adapter injects the canonical payload (`tenant`, `provider`, `provider_ids`, `session`, `timestamp`, `text`, `attachments`, `buttons`, `entities`, `metadata`, `channel_data`, `raw`) and uses the same session-key policy `{tenant}:{provider}:{conversation-or-thread-or-channel}:{user}` enforced everywhere. Custom adapters can follow the same pattern by translating incoming payloads into an `IngressEnvelope`.

## License

This project is licensed under the [MIT License](./LICENSE).
