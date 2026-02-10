import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { HAClient } from "./ha-client.js";
import type { HAEntityState } from "./types.js";

function textResult(data: unknown) {
  return { content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }] };
}

const USEFUL_ATTRIBUTES = [
  "brightness", "color_temp", "rgb_color",
  "unit_of_measurement", "device_class",
  "current_temperature", "temperature", "hvac_action",
] as const;

function summarizeState(entity: HAEntityState) {
  const extra: Record<string, unknown> = {};
  for (const key of USEFUL_ATTRIBUTES) {
    if (entity.attributes[key] !== undefined) {
      extra[key] = entity.attributes[key];
    }
  }
  return {
    entity_id: entity.entity_id,
    state: entity.state,
    friendly_name: entity.attributes["friendly_name"] ?? entity.entity_id,
    ...extra,
  };
}

export function createServer(ha: HAClient): McpServer {
  const server = new McpServer({
    name: "smarthome-mcp",
    version: "1.0.0",
  });

  // ── Light controls ──────────────────────────────────────────────

  server.tool(
    "light.turn_on",
    "Turn on a light entity",
    { entity_id: z.string().describe("The light entity id, e.g. light.kitchen") },
    async ({ entity_id }) => {
      const result = await ha.callService("light", "turn_on", { entity_id });
      return textResult(result.map(summarizeState));
    }
  );

  server.tool(
    "light.turn_off",
    "Turn off a light entity",
    { entity_id: z.string().describe("The light entity id, e.g. light.kitchen") },
    async ({ entity_id }) => {
      const result = await ha.callService("light", "turn_off", { entity_id });
      return textResult(result.map(summarizeState));
    }
  );

  server.tool(
    "light.set_brightness",
    "Set the brightness of a light (0–255)",
    {
      entity_id: z.string().describe("The light entity id"),
      brightness: z.number().min(0).max(255).describe("Brightness level 0–255"),
    },
    async ({ entity_id, brightness }) => {
      const result = await ha.callService("light", "turn_on", {
        entity_id,
        brightness,
      });
      return textResult(result.map(summarizeState));
    }
  );

  // ── Area-based light controls ───────────────────────────────────

  server.tool(
    "light.turn_on_in_area",
    "Turn on all lights in a named area (e.g. 'kitchen', 'living_room')",
    { area: z.string().describe("Area id as known in Home Assistant") },
    async ({ area }) => {
      const entities = await ha.getEntitiesInArea(area);
      const lights = entities.filter((e) => e.startsWith("light."));
      if (lights.length === 0) {
        return textResult({ message: `No lights found in area '${area}'` });
      }
      const result = await ha.callService("light", "turn_on", {
        entity_id: lights,
      });
      return textResult(result.map(summarizeState));
    }
  );

  server.tool(
    "light.turn_off_in_area",
    "Turn off all lights in a named area",
    { area: z.string().describe("Area id as known in Home Assistant") },
    async ({ area }) => {
      const entities = await ha.getEntitiesInArea(area);
      const lights = entities.filter((e) => e.startsWith("light."));
      if (lights.length === 0) {
        return textResult({ message: `No lights found in area '${area}'` });
      }
      const result = await ha.callService("light", "turn_off", {
        entity_id: lights,
      });
      return textResult(result.map(summarizeState));
    }
  );

  // ── State queries ───────────────────────────────────────────────

  server.tool(
    "entity.get_state",
    "Get the current state and attributes of any entity",
    { entity_id: z.string().describe("Entity id, e.g. sensor.temperature") },
    async ({ entity_id }) => {
      const state = await ha.getState(entity_id);
      return textResult(summarizeState(state));
    }
  );

  server.tool(
    "area.get_status",
    "Get the state of all entities in an area",
    { area: z.string().describe("Area id") },
    async ({ area }) => {
      const entityIds = await ha.getEntitiesInArea(area);
      const states = await Promise.all(
        entityIds.map(async (id) => {
          const s = await ha.getState(id);
          return summarizeState(s);
        })
      );
      return textResult({ area, entities: states });
    }
  );

  server.tool(
    "area.list",
    "List all areas configured in Home Assistant",
    {},
    async () => {
      const areaIds = await ha.getAreas();
      const areas = await Promise.all(
        areaIds.map(async (id) => ({
          id,
          name: await ha.getAreaName(id),
        }))
      );
      return textResult(areas);
    }
  );

  // ── Entity discovery ─────────────────────────────────────────────

  server.tool(
    "entity.list",
    "List all entities for a domain (e.g. light, sensor, switch, climate) with their current states. Optionally filter by device_class (e.g. temperature, humidity, motion).",
    {
      domain: z.string().describe("Entity domain: light, sensor, switch, binary_sensor, climate, etc."),
      device_class: z.string().optional().describe("Filter by device_class attribute, e.g. 'temperature', 'humidity', 'motion'"),
      state: z.string().optional().describe("Filter by state value, e.g. 'on', 'off'. Omit to return all."),
    },
    async ({ domain, device_class, state }) => {
      let entities = await ha.getStatesByDomain(domain);
      if (device_class) {
        entities = entities.filter((e) => e.attributes["device_class"] === device_class);
      }
      if (state) {
        entities = entities.filter((e) => e.state === state);
      }
      const metaMap = await ha.getEntityMetaMap(entities.map((e) => e.entity_id));
      return textResult(entities.map((e) => ({
        ...summarizeState(e),
        area: metaMap[e.entity_id]?.area ?? null,
        device: metaMap[e.entity_id]?.device ?? null,
      })));
    }
  );

  server.tool(
    "entity.search",
    "Search entities by keyword across entity IDs and friendly names. Returns matching entities with their current states.",
    {
      query: z.string().describe("Search term to match against entity_id and friendly_name"),
      domain: z.string().optional().describe("Optionally restrict to a domain (light, sensor, etc.)"),
    },
    async ({ query, domain }) => {
      const states = domain ? await ha.getStatesByDomain(domain) : await ha.getStates();
      const q = query.toLowerCase();
      const matches = states.filter((e) => {
        const name = String(e.attributes["friendly_name"] ?? "").toLowerCase();
        return e.entity_id.toLowerCase().includes(q) || name.includes(q);
      });
      const metaMap = await ha.getEntityMetaMap(matches.map((e) => e.entity_id));
      return textResult(matches.map((e) => ({
        ...summarizeState(e),
        area: metaMap[e.entity_id]?.area ?? null,
        device: metaMap[e.entity_id]?.device ?? null,
      })));
    }
  );

  // ── Todo lists ───────────────────────────────────────────────────

  server.tool(
    "todo.get_items",
    "Get items from a Home Assistant todo list. Use entity.list with domain 'todo' to discover available lists first.",
    {
      entity_id: z.string().describe("Todo list entity id, e.g. todo.shopping_list"),
      status: z.enum(["needs_action", "completed"]).optional().describe("Filter by status. Omit to return all items."),
    },
    async ({ entity_id, status }) => {
      const response = await ha.callServiceWithResponse("todo", "get_items", {
        entity_id,
        ...(status && { status }),
      });
      return textResult(response);
    }
  );

  server.tool(
    "todo.add_item",
    "Add an item to a Home Assistant todo list",
    {
      entity_id: z.string().describe("Todo list entity id, e.g. todo.shopping_list"),
      item: z.string().describe("The item text to add"),
    },
    async ({ entity_id, item }) => {
      const result = await ha.callService("todo", "add_item", { entity_id, item });
      return textResult({ added: item, list: entity_id, state: result });
    }
  );

  server.tool(
    "todo.update_item",
    "Update a todo item's text or status (mark as completed/needs_action). Use todo.get_items first to find the item name.",
    {
      entity_id: z.string().describe("Todo list entity id"),
      item: z.string().describe("Current item text (must match exactly)"),
      rename: z.string().optional().describe("New text for the item"),
      status: z.enum(["needs_action", "completed"]).optional().describe("New status"),
    },
    async ({ entity_id, item, rename, status }) => {
      const result = await ha.callService("todo", "update_item", {
        entity_id,
        item,
        ...(rename && { rename }),
        ...(status && { status }),
      });
      return textResult({ updated: item, list: entity_id, state: result });
    }
  );

  server.tool(
    "todo.remove_item",
    "Remove an item from a Home Assistant todo list",
    {
      entity_id: z.string().describe("Todo list entity id"),
      item: z.string().describe("The item text to remove (must match exactly)"),
    },
    async ({ entity_id, item }) => {
      const result = await ha.callService("todo", "remove_item", { entity_id, item });
      return textResult({ removed: item, list: entity_id, state: result });
    }
  );

  // ── Generic service call ────────────────────────────────────────

  server.tool(
    "service.call",
    "Call any Home Assistant service (escape hatch for anything not covered by other tools)",
    {
      domain: z.string().describe("Service domain, e.g. 'switch', 'climate'"),
      service: z.string().describe("Service name, e.g. 'turn_on', 'set_temperature'"),
      data: z
        .record(z.unknown())
        .optional()
        .describe("Service data payload"),
    },
    async ({ domain, service, data }) => {
      const result = await ha.callService(domain, service, data);
      return textResult(result);
    }
  );

  return server;
}
