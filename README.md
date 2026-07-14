# smarthome-mcp

MCP server that connects Claude Code (or any MCP client) to **Home Assistant** and **Zigbee2MQTT**. Written in Rust, ships as a single static binary under 15MB.

## Backends

### Home Assistant (REST API)

| Tool                        | Scope        | Description                                              |
| --------------------------- | ------------ | -------------------------------------------------------- |
| `ha_entity.get_state`       | `ha:read`    | Get state of any entity                                  |
| `ha_entity.list`            | `ha:read`    | List entities by domain, filter by device_class or state |
| `ha_entity.search`          | `ha:read`    | Search entities by keyword                               |
| `ha_area.list`              | `ha:read`    | List all configured areas                                |
| `ha_area.get_status`        | `ha:read`    | Get all entity states in an area                         |
| `ha_todo.get_items`         | `ha:read`    | Get items from a todo list                               |
| `ha_light.turn_on`          | `ha:control` | Turn on a light entity                                   |
| `ha_light.turn_off`         | `ha:control` | Turn off a light entity                                  |
| `ha_light.set_brightness`   | `ha:control` | Set brightness (0-255)                                   |
| `ha_light.turn_on_in_area`  | `ha:control` | Turn on all lights in an area                            |
| `ha_light.turn_off_in_area` | `ha:control` | Turn off all lights in an area                           |
| `ha_todo.add_item`          | `ha:control` | Add item to a todo list                                  |
| `ha_todo.update_item`       | `ha:control` | Rename or change status of a todo item                   |
| `ha_todo.remove_item`       | `ha:control` | Remove item from a todo list                             |
| `ha_service.call`           | `ha:control` | Generic escape hatch for any HA service                  |

### Zigbee2MQTT (MQTT)

| Tool                    | Scope         | Description                                                    |
| ----------------------- | ------------- | -------------------------------------------------------------- |
| `z2m_device.list`       | `z2m:read`    | List all Zigbee devices with their exposed features            |
| `z2m_device.get_state`  | `z2m:read`    | Get current state of a device (includes availability)          |
| `z2m_group.list`        | `z2m:read`    | List all Z2M groups                                            |
| `z2m_bridge.info`       | `z2m:read`    | Get bridge info (coordinator, version, network)                |
| `z2m_device.set`        | `z2m:control` | Set device state; validated against the device's exposes       |
| `z2m_device.rename`     | `z2m:control` | Rename a device                                                |
| `z2m_group.add`         | `z2m:control` | Create a Z2M group                                             |
| `z2m_bridge.permit_join`| `z2m:control` | Enable/disable permit join                                     |
| `z2m_bridge.networkmap` | `z2m:control` | Request network map                                            |

`z2m_device.set` validates payloads against each device's `exposes` definition before sending. Unknown fields and out-of-range values are rejected with a helpful error.

## Setup

### Prerequisites

- A Home Assistant instance with a [long-lived access token](https://developers.home-assistant.io/docs/auth_api/#long-lived-access-token) (for HA backend)
- A Zigbee2MQTT instance with MQTT broker access (for Z2M backend)
- Either backend is optional; configure one or both

### Run with Docker

```sh
docker run -d \
  -p 3000:3000 \
  -e HA_URL=http://homeassistant.local:8123 \
  -e HA_TOKEN=your_token \
  -e Z2M_MQTT_HOST=mqtt://localhost:1883 \
  ghcr.io/alkj/smarthome-mcp:latest
```

Or with a config file:

```sh
docker run -d \
  -p 3000:3000 \
  -v ./config.toml:/config.toml \
  -e CONFIG_FILE=/config.toml \
  ghcr.io/alkj/smarthome-mcp:latest
```

### Run locally

```sh
# With env vars
HA_URL=http://homeassistant.local:8123 HA_TOKEN=your_token cargo run

# Or with config file
CONFIG_FILE=config.toml cargo run
```

### Build from source

```sh
cargo build --release
```

### Claude Code config

Add to `.mcp.json` in your project (or `~/.claude/settings.json` for global):

```json
{
  "mcpServers": {
    "smarthome": {
      "type": "http",
      "url": "http://localhost:3000/mcp"
    }
  }
}
```

## Configuration

Configure via environment variables, a TOML config file, or both. Env vars override TOML values.

### Environment variables

```sh
# Home Assistant (omit to disable)
HA_URL=http://homeassistant.local:8123
HA_TOKEN=your_long_lived_access_token

# Zigbee2MQTT (omit to disable)
Z2M_MQTT_HOST=mqtt://localhost:1883
Z2M_MQTT_USER=              # optional
Z2M_MQTT_PASS=              # optional
Z2M_BASE_TOPIC=zigbee2mqtt  # default: zigbee2mqtt

# Server
PORT=3000                   # default: 3000
CONFIG_FILE=config.toml     # optional path to TOML config
```

### TOML config

See [config.example.toml](config.example.toml) for a full example with auth rules.

## Auth

Auth is optional. Without an `[auth]` section, all requests are allowed (open mode for local-only setups).

Three strategies, evaluated in order (first match wins):

1. **IP whitelist** - grant scopes to requests from configured CIDR ranges, no token needed
2. **Static tokens** - bearer tokens defined in config, each with its own scopes
3. **Proxy** - validate the bearer token against HA's REST API or the MQTT broker

### Scopes

| Scope         | Access                                                   |
| ------------- | -------------------------------------------------------- |
| `ha:read`     | Entity/area state queries, listing, search               |
| `ha:control`  | Light control, todo management, `ha_service.call`        |
| `z2m:read`    | Device/group listing, state queries, bridge info         |
| `z2m:control` | Device set/rename, group management, permit join         |
| `*`           | All scopes                                               |

Example: give your local network full access, external clients read-only:

```toml
[auth]

[[auth.whitelist]]
cidrs = ["192.168.0.0/16", "10.0.0.0/8"]
scopes = ["*"]

[[auth.tokens]]
token = "readonly-external-token"
scopes = ["ha:read", "z2m:read"]
```

## Health endpoint

`GET /health` returns backend status:

```json
{
  "status": "ok",
  "backends": {
    "ha": { "status": "ok" },
    "z2m": { "status": "ok" }
  }
}
```

`status` is `"ok"` when all configured backends are reachable, `"degraded"` when at least one is down, `"unavailable"` when all are down. The server starts and serves requests even when backends are unreachable.

## Architecture

- **MCP transport**: [rmcp](https://crates.io/crates/rmcp) with streamable HTTP via axum
- **HA client**: reqwest with 10s timeouts
- **Z2M client**: rumqttc with in-memory device cache from retained `bridge/devices` topic
- **Docker**: multi-arch (amd64 + arm64) static musl binary on scratch

## Notes

- The HA backend uses HA's [template API](https://developers.home-assistant.io/docs/api/rest/#post-apitemplate) for area/device discovery since the REST API has no direct areas endpoint.
- Z2M device payloads are validated against the device's `exposes` definition. Use `z2m_device.list` to see what each device accepts.
- The Z2M MQTT client reconnects automatically on connection loss.
