# smarthome-mcp

MCP server that connects Claude Code (or any MCP client) to Home Assistant via its REST API. Runs as a standalone HTTP server with streamable HTTP transport.

## Tools

| Tool                        | Description                                              |
| --------------------------- | -------------------------------------------------------- |
| `ha_light.turn_on`          | Turn on a light entity                                   |
| `ha_light.turn_off`         | Turn off a light entity                                  |
| `ha_light.set_brightness`   | Set brightness (0-255)                                   |
| `ha_light.turn_on_in_area`  | Turn on all lights in an area                            |
| `ha_light.turn_off_in_area` | Turn off all lights in an area                           |
| `ha_entity.get_state`       | Get state of any entity                                  |
| `ha_entity.list`            | List entities by domain, filter by device_class or state |
| `ha_entity.search`          | Search entities by keyword                               |
| `ha_area.get_status`        | Get all entity states in an area                         |
| `ha_area.list`              | List all configured areas                                |
| `ha_todo.get_items`         | Get items from a todo list                               |
| `ha_todo.add_item`          | Add item to a todo list                                  |
| `ha_todo.update_item`       | Rename or change status of a todo item                   |
| `ha_todo.remove_item`       | Remove item from a todo list                             |
| `ha_service.call`           | Generic escape hatch for any HA service                  |

## Setup

### Prerequisites

- Node.js 20+
- A Home Assistant instance with a [long-lived access token](https://developers.home-assistant.io/docs/auth_api/#long-lived-access-token)

### Environment variables

```sh
HA_URL=http://homeassistant.local:8123
HA_TOKEN=your_long_lived_access_token
PORT=3000
```

### Run locally

```sh
cp env.example .env
# fill in HA_URL and HA_TOKEN

corepack enable pnpm
pnpm install
pnpm dev
```

### Run with Docker Compose

```yaml
services:
  smarthome-mcp:
    build: .
    ports:
      - "3000:3000"
    environment:
      - HA_URL=http://homeassistant.local:8123
      - HA_TOKEN=${HA_TOKEN}
      - PORT=3000
    restart: unless-stopped
```

```sh
docker compose up --build
```

### Claude Code config

Add to `.mcp.json` in your project (or `~/.claude/settings.json` for global):

```json
{
  "mcpServers": {
    "home-assistant": {
      "type": "http",
      "url": "http://localhost:3000/mcp"
    }
  }
}
```

## Notes

The server uses HA's [template API](https://developers.home-assistant.io/docs/api/rest/#post-apitemplate) for area/device discovery since the REST API has no direct areas endpoint.
