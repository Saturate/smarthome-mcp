# Spec: smarthome-mcp v2 (Rust rewrite)

## Objective

Rewrite the smarthome-mcp server in Rust. The server is an MCP proxy that lets Claude Code (or any MCP client) control a smart home through two backends: Home Assistant (REST API) and Zigbee2MQTT (MQTT). The current TypeScript implementation (~760 lines) covers HA only; the Rust version adds Z2M support, an auth layer, and ships as a minimal static binary.

**Users:** developers running Claude Code locally or on a server, connecting to their home automation setup.

**Success looks like:** a single multi-arch Docker image under 15MB that proxies MCP tool calls to HA and Z2M with configurable auth.

## Tech Stack

| Component | Choice | Why |
|---|---|---|
| Language | Rust (2024 edition) | Small binary, no runtime |
| MCP SDK | `rmcp` 0.16.x | Official Rust MCP SDK, streamable HTTP via axum, `#[tool]` proc macros |
| HTTP framework | `axum` (via rmcp) | Comes with rmcp's streamable HTTP transport |
| HA client | `reqwest` | Async HTTP client with connection pooling |
| MQTT client | `rumqttc` v5 | Async MQTT 3.1.1/5 client, well-maintained |
| Serialization | `serde` + `serde_json` | Standard |
| Schema gen | `schemars` | Required by rmcp's `#[tool]` macro for JSON Schema |
| Config | `envy` + optional TOML (`toml`) | Env vars primary, config file optional |
| Logging | `tracing` + `tracing-subscriber` | Structured logging, integrates with tokio |
| Build target | `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` | Static binaries for scratch Docker image |

## Commands

```sh
# Dev
cargo run

# Build (release, native arch)
cargo build --release

# Build (cross-compile for linux arm64)
cross build --release --target aarch64-unknown-linux-musl

# Test
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt --check

# Docker (multi-arch)
docker buildx build --platform linux/amd64,linux/arm64 -t smarthome-mcp .
```

## Project Structure

```
smarthome-mcp/
├── Cargo.toml
├── Cargo.lock
├── Dockerfile                 # multi-stage, multi-arch
├── .github/workflows/
│   ├── ci.yml                 # clippy, fmt, test
│   └── release.yml            # cross-build + multi-arch Docker push to GHCR
├── config.example.toml        # example config file
├── src/
│   ├── main.rs                # entrypoint, config loading, server startup
│   ├── config.rs              # config structs (env + TOML), auth config
│   ├── auth/
│   │   ├── mod.rs             # auth middleware, strategy dispatcher, scope types
│   │   ├── scopes.rs          # scope definitions, tool-to-scope mapping
│   │   ├── ip_whitelist.rs    # IP/CIDR range matching
│   │   ├── token_static.rs    # static tokens from config
│   │   └── token_proxy.rs     # validate token against HA or MQTT broker
│   ├── ha/
│   │   ├── mod.rs             # re-exports
│   │   ├── client.rs          # reqwest-based HA REST client
│   │   └── tools.rs           # MCP tool definitions for HA
│   ├── z2m/
│   │   ├── mod.rs             # re-exports
│   │   ├── client.rs          # MQTT client, topic subscriptions, device cache
│   │   └── tools.rs           # MCP tool definitions for Z2M
│   └── util.rs                # shared helpers (jinja list parser, etc.)
└── tests/
    ├── ha_client_test.rs
    ├── z2m_client_test.rs
    └── auth_test.rs
```

## Code Style

Rust 2024 edition, stable toolchain. Clippy with `-D warnings`. Format with `rustfmt`.

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct LightTurnOnParams {
    #[schemars(description = "The light entity id, e.g. light.kitchen")]
    entity_id: String,
}

#[tool(description = "Turn on a light entity")]
async fn ha_light_turn_on(
    &self,
    Parameters(params): Parameters<LightTurnOnParams>,
) -> Result<CallToolResult, McpError> {
    let result = self.ha.call_service("light", "turn_on", json!({"entity_id": params.entity_id})).await?;
    Ok(CallToolResult::success(serde_json::to_string_pretty(&result)?))
}
```

## Config

Primary config is env vars. Optional TOML file for complex auth rules.

### Env vars

Env vars handle the simple case. For scoped auth rules, use the TOML config.

```sh
# Home Assistant (required if HA backend enabled)
HA_URL=http://homeassistant.local:8123
HA_TOKEN=your_long_lived_access_token

# Zigbee2MQTT (required if Z2M backend enabled)
Z2M_MQTT_HOST=mqtt://localhost:1883
Z2M_MQTT_USER=           # optional
Z2M_MQTT_PASS=           # optional
Z2M_BASE_TOPIC=zigbee2mqtt   # default: zigbee2mqtt

# Server
PORT=3000                    # default: 3000
CONFIG_FILE=config.toml      # optional, path to TOML config
```

### TOML config (`config.toml`)

TOML is the config file format. Env vars override TOML values for backend connection settings. Auth scopes are TOML-only since env vars can't express them cleanly.

```toml
[server]
port = 3000

[ha]
url = "http://homeassistant.local:8123"
token = "your_token"

[z2m]
mqtt_host = "mqtt://localhost:1883"
base_topic = "zigbee2mqtt"

# Scopes control which tool groups a client can access.
# Available scopes:
#   ha:read      - entity/area state queries, listing, search
#   ha:control   - light control, todo management, service.call
#   z2m:read     - device/group listing, state queries, bridge info
#   z2m:control  - device set, rename, group management, permit_join
#   *            - all scopes

[auth]
# If no auth section is present, all requests are allowed (open mode).
# Strategies are evaluated in order; first match wins.

# IP whitelist: grant scopes without requiring a token
[[auth.whitelist]]
cidrs = ["192.168.0.0/16", "10.0.0.0/8"]
scopes = ["*"]

[[auth.whitelist]]
cidrs = ["172.16.0.0/12"]
scopes = ["ha:read", "z2m:read"]

# Static tokens: defined inline with scopes
[[auth.tokens]]
token = "full-access-token-here"
scopes = ["*"]

[[auth.tokens]]
token = "readonly-token-here"
scopes = ["ha:read", "z2m:read"]

# Proxy auth: validate bearer token against an external system
[auth.proxy]
# "ha" validates against HA REST API, "mqtt" against the MQTT broker
backend = "ha"        # "ha" | "mqtt"
scopes = ["*"]        # scopes granted on successful validation
cache_ttl = 300       # seconds to cache a valid token (0 = no cache)
```

Either backend (HA, Z2M) can be disabled by omitting its config section.

## Auth Layer

Auth is axum middleware, evaluated before requests reach the MCP transport. Each strategy resolves to a set of **scopes** that determine which tools the client can call.

### Scopes

| Scope | Tools |
|---|---|
| `ha:read` | `ha_entity.get_state`, `ha_entity.list`, `ha_entity.search`, `ha_area.get_status`, `ha_area.list`, `ha_todo.get_items` |
| `ha:control` | `ha_light.turn_on`, `ha_light.turn_off`, `ha_light.set_brightness`, `ha_light.turn_on_in_area`, `ha_light.turn_off_in_area`, `ha_todo.add_item`, `ha_todo.update_item`, `ha_todo.remove_item`, `ha_service.call` |
| `z2m:read` | `z2m_device.list`, `z2m_device.get_state`, `z2m_group.list`, `z2m_bridge.info` |
| `z2m:control` | `z2m_device.set`, `z2m_device.rename`, `z2m_group.add`, `z2m_bridge.permit_join`, `z2m_bridge.networkmap` |
| `*` | All of the above |

### Evaluation order (first match wins)

1. **IP whitelist** - if client IP matches a configured CIDR range, grant the scopes assigned to that range
2. **Static token** - if `Authorization: Bearer <token>` matches a configured token, grant its scopes
3. **Proxy** - forward the bearer token to HA or MQTT broker for validation; on success, grant configured scopes; cache valid tokens per TTL

If no auth section is present in config, all requests are allowed (open mode for local-only setups).

When a client calls a tool they lack the scope for, the server returns a JSON-RPC error with code `-32001` and a message naming the missing scope.

## MCP Tools

### HA tools (same as current, `ha_` prefix)

| Tool | Description |
|---|---|
| `ha_light.turn_on` | Turn on a light entity |
| `ha_light.turn_off` | Turn off a light entity |
| `ha_light.set_brightness` | Set brightness (0-255) |
| `ha_light.turn_on_in_area` | Turn on all lights in an area |
| `ha_light.turn_off_in_area` | Turn off all lights in an area |
| `ha_entity.get_state` | Get state of any entity |
| `ha_entity.list` | List entities by domain, filter by device_class or state |
| `ha_entity.search` | Search entities by keyword |
| `ha_area.get_status` | Get all entity states in an area |
| `ha_area.list` | List all configured areas |
| `ha_todo.get_items` | Get items from a todo list |
| `ha_todo.add_item` | Add item to a todo list |
| `ha_todo.update_item` | Rename or change status of a todo item |
| `ha_todo.remove_item` | Remove item from a todo list |
| `ha_service.call` | Generic escape hatch for any HA service |

### Z2M tools (new, `z2m_` prefix)

| Tool | Description |
|---|---|
| `z2m_device.list` | List all Zigbee devices with their exposed features (from bridge/devices retained topic) |
| `z2m_device.get_state` | Get current state of a device (includes availability) |
| `z2m_device.set` | Set device state; payload validated against the device's exposes definition |
| `z2m_device.rename` | Rename a device via bridge request |
| `z2m_group.list` | List all Z2M groups |
| `z2m_group.add` | Create a Z2M group |
| `z2m_bridge.info` | Get bridge info (coordinator, version, network) |
| `z2m_bridge.permit_join` | Enable/disable permit join |
| `z2m_bridge.networkmap` | Request network map |

### Z2M client internals

The Z2M client maintains an in-memory device cache by subscribing to `{base_topic}/bridge/devices` (retained). Each cached device includes its `exposes` definition (features, types, value ranges, access modes).

**MQTT QoS:** QoS 1 for publishes (control commands), QoS 0 for subscriptions (state reads).

On tool calls:

- **State queries** subscribe to `{base_topic}/{device_name}` and read the last retained message. Response includes the device's availability status from `{base_topic}/{device_name}/availability`.
- **Control (`z2m_device.set`)** validates the payload against the device's `exposes` definition before publishing. Unknown fields or out-of-range values are rejected with an error listing valid fields and ranges. If the device is unavailable, the command is still sent (Zigbee devices may accept queued commands on wake-up) but the response includes an availability warning.
- **Device listing** returns all devices with their exposed features, so the MCP client knows what each device accepts before calling `z2m_device.set`.
- **Bridge commands** use the request/response pattern: publish to `{base_topic}/bridge/request/{cmd}`, await response on `{base_topic}/bridge/response/{cmd}`

## Health and Graceful Degradation

The server starts even if one or both backends are unreachable. Backend availability is tracked at runtime and reflected in the health endpoint and tool behavior.

### `/health` endpoint

```json
{
  "status": "degraded",
  "backends": {
    "ha": { "status": "ok", "url": "http://homeassistant.local:8123" },
    "z2m": { "status": "unavailable", "error": "MQTT connection refused" }
  },
  "sessions": 2
}
```

Top-level `status` is `"ok"` when all configured backends are reachable, `"degraded"` when at least one is down, and `"unavailable"` when all are down.

### Tool behavior when a backend is down

- Tools for an unavailable backend return a JSON-RPC error with code `-32002` and a message like `"z2m backend is currently unavailable"`.
- The `tools/list` MCP response still includes all configured tools (so clients know what exists), but unavailable tools include a note in their description.
- The server retries backend connections in the background with exponential backoff. When a backend recovers, its tools become available again without a restart.

## Testing Strategy

- **Unit tests** (`cargo test`): test each module in isolation. Mock HTTP responses for HA client, mock MQTT broker for Z2M client. Auth middleware tested with synthetic requests.
- **Integration tests** (`tests/`): test tool registration and invocation through the MCP server. Use a mock HA server (axum test server) and a mock MQTT broker.
- **CI**: clippy, fmt check, all tests on every push/PR.
- **Manual testing**: connect Claude Code to the running server and exercise tools against a real HA + Z2M setup.

## Docker

Multi-stage Dockerfile:

```dockerfile
# Stage 1: build (per-arch)
FROM rust:1-slim AS build
ARG TARGETARCH
# Install musl toolchain for target arch
# cargo build --release --target {arch}-unknown-linux-musl

# Stage 2: runtime
FROM scratch
COPY --from=build /app/target/*/release/smarthome-mcp /smarthome-mcp
EXPOSE 3000
ENTRYPOINT ["/smarthome-mcp"]
```

Built with `docker buildx` for `linux/amd64` and `linux/arm64`. CI pushes to GHCR on version tags.

**Target image size:** under 15MB (static binary, no OS, no runtime).

## Boundaries

- **Always:** run `cargo clippy` and `cargo test` before commits. Validate all external input at boundaries (MCP params, HA responses, MQTT payloads). Use timeouts on all external calls.
- **Ask first:** adding new dependencies, changing the MCP tool surface, modifying auth behavior.
- **Never:** store credentials in code or logs. Log tokens or passwords. Skip auth checks for non-whitelisted IPs.

## Success Criteria

1. All 15 existing HA tools work identically to the TypeScript version
2. All 9 Z2M tools connect and operate against a real Zigbee2MQTT instance
3. Scoped auth layer correctly enforces configured strategies and scopes (unit tested)
4. Docker image under 15MB for each arch
5. `cargo clippy -- -D warnings` passes clean
6. CI builds and tests on both amd64 and arm64
7. Connects to Claude Code via streamable HTTP and responds to tool calls
8. Server starts with degraded status when a backend is unreachable; `/health` reflects availability
9. Tools for unavailable backends return clear errors, recover without restart when backend comes back

## Open Questions

None. All resolved.
