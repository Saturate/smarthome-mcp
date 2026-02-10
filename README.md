# smarthome-mcp

MCP server that connects Claude Code (or any MCP client) to Home Assistant via its REST API. Runs as a standalone HTTP server with streamable HTTP transport.

## Tools

| Tool                     | Description                                              |
| ------------------------ | -------------------------------------------------------- |
| `light.turn_on`          | Turn on a light entity                                   |
| `light.turn_off`         | Turn off a light entity                                  |
| `light.set_brightness`   | Set brightness (0-255)                                   |
| `light.turn_on_in_area`  | Turn on all lights in an area                            |
| `light.turn_off_in_area` | Turn off all lights in an area                           |
| `entity.get_state`       | Get state of any entity                                  |
| `entity.list`            | List entities by domain, filter by device_class or state |
| `entity.search`          | Search entities by keyword                               |
| `area.get_status`        | Get all entity states in an area                         |
| `area.list`              | List all configured areas                                |
| `todo.get_items`         | Get items from a todo list                               |
| `todo.add_item`          | Add item to a todo list                                  |
| `todo.update_item`       | Rename or change status of a todo item                   |
| `todo.remove_item`       | Remove item from a todo list                             |
| `service.call`           | Generic escape hatch for any HA service                  |

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
