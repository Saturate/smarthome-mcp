import { describe, expect, it } from "vitest";
import { HAClient, parseJinjaList } from "./ha-client.js";

describe("HAClient constructor", () => {
  it("strips trailing slash from URL", () => {
    const client = new HAClient({ url: "http://ha.local:8123/", token: "abc" });
    // baseUrl is private, so verify indirectly via the string representation
    expect(JSON.stringify(client)).toContain("http://ha.local:8123");
    expect(JSON.stringify(client)).not.toContain("http://ha.local:8123/");
  });

  it("keeps URL without trailing slash unchanged", () => {
    const client = new HAClient({ url: "http://ha.local:8123", token: "abc" });
    expect(JSON.stringify(client)).toContain("http://ha.local:8123");
  });

  it("sets Authorization header from token", () => {
    const client = new HAClient({
      url: "http://ha.local:8123",
      token: "test-token-123",
    });
    expect(JSON.stringify(client)).toContain("Bearer test-token-123");
  });
});

describe("parseJinjaList", () => {
  it("returns empty array for empty list", () => {
    expect(parseJinjaList("[]")).toEqual([]);
  });

  it("parses single-quoted items", () => {
    expect(parseJinjaList("['kitchen', 'living_room']")).toEqual([
      "kitchen",
      "living_room",
    ]);
  });

  it("parses double-quoted items", () => {
    expect(parseJinjaList('["kitchen", "living_room"]')).toEqual([
      "kitchen",
      "living_room",
    ]);
  });

  it("handles whitespace around items", () => {
    expect(parseJinjaList("[  'kitchen'  ,  'bedroom'  ]")).toEqual([
      "kitchen",
      "bedroom",
    ]);
  });

  it("handles a single element", () => {
    expect(parseJinjaList("['only_one']")).toEqual(["only_one"]);
  });

  it("handles whitespace around the entire string", () => {
    expect(parseJinjaList("  ['a', 'b']  ")).toEqual(["a", "b"]);
  });
});
