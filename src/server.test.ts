import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { describe, expect, it } from "vitest";
import { HAClient } from "./ha-client.js";
import { createServer } from "./server.js";

const EXPECTED_TOOLS = [
  "ha_light.turn_on",
  "ha_light.turn_off",
  "ha_light.set_brightness",
  "ha_light.turn_on_in_area",
  "ha_light.turn_off_in_area",
  "ha_entity.get_state",
  "ha_area.get_status",
  "ha_area.list",
  "ha_entity.list",
  "ha_entity.search",
  "ha_todo.get_items",
  "ha_todo.add_item",
  "ha_todo.update_item",
  "ha_todo.remove_item",
  "ha_service.call",
] as const;

function makeDummyClient(): HAClient {
  return new HAClient({ url: "http://localhost:8123", token: "fake" });
}

describe("createServer", () => {
  it("returns an McpServer instance", () => {
    const server = createServer(makeDummyClient());
    expect(server).toBeInstanceOf(McpServer);
  });

  it("registers all 15 expected tools", async () => {
    const server = createServer(makeDummyClient());

    const [clientTransport, serverTransport] =
      InMemoryTransport.createLinkedPair();
    const client = new Client({ name: "test-client", version: "1.0.0" });

    await Promise.all([
      client.connect(clientTransport),
      server.connect(serverTransport),
    ]);

    const { tools } = await client.listTools();
    const toolNames = tools.map((t) => t.name).sort();

    expect(toolNames).toEqual([...EXPECTED_TOOLS].sort());

    await client.close();
  });
});
