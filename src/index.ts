import "dotenv/config";
import { randomUUID } from "node:crypto";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import { isInitializeRequest } from "@modelcontextprotocol/sdk/types.js";
import cors from "cors";
import express from "express";
import { HAClient } from "./ha-client.js";
import { createServer } from "./server.js";

const HA_URL = process.env.HA_URL;
const HA_TOKEN = process.env.HA_TOKEN;
const PORT = Number(process.env.PORT ?? 3000);

if (!HA_URL || !HA_TOKEN) {
  console.error("Missing required env vars: HA_URL and HA_TOKEN");
  process.exit(1);
}

const ha = new HAClient({ url: HA_URL, token: HA_TOKEN });

const app = express();
app.use(cors());
app.use(express.json());

const transports: Record<string, StreamableHTTPServerTransport> = {};

app.post("/mcp", async (req, res) => {
  const sessionId = req.headers["mcp-session-id"] as string | undefined;

  if (sessionId && transports[sessionId]) {
    await transports[sessionId].handleRequest(req, res, req.body);
    return;
  }

  // Stale/unknown session — 404 tells the client to re-initialize
  if (sessionId && !transports[sessionId]) {
    res.status(404).json({
      jsonrpc: "2.0",
      error: { code: -32001, message: "Session not found. Re-initialize." },
      id: null,
    });
    return;
  }

  if (!sessionId && isInitializeRequest(req.body)) {
    const transport = new StreamableHTTPServerTransport({
      sessionIdGenerator: () => randomUUID(),
      onsessioninitialized: (id) => {
        transports[id] = transport;
      },
    });

    transport.onclose = () => {
      const id = transport.sessionId;
      if (id) delete transports[id];
    };

    const server = createServer(ha);
    await server.connect(transport);
    await transport.handleRequest(req, res, req.body);
    return;
  }

  res.status(400).json({
    jsonrpc: "2.0",
    error: {
      code: -32000,
      message: "Bad Request: no valid session or initialize request",
    },
    id: null,
  });
});

app.get("/mcp", async (req, res) => {
  const sessionId = req.headers["mcp-session-id"] as string | undefined;
  if (!sessionId || !transports[sessionId]) {
    res.status(400).json({
      jsonrpc: "2.0",
      error: {
        code: -32000,
        message: "Bad Request: missing or invalid session",
      },
      id: null,
    });
    return;
  }
  // SSE stream for server-initiated messages
  await transports[sessionId].handleRequest(req, res);
});

app.delete("/mcp", async (req, res) => {
  const sessionId = req.headers["mcp-session-id"] as string | undefined;
  if (!sessionId || !transports[sessionId]) {
    res.status(400).json({
      jsonrpc: "2.0",
      error: {
        code: -32000,
        message: "Bad Request: missing or invalid session",
      },
      id: null,
    });
    return;
  }
  await transports[sessionId].handleRequest(req, res);
});

app.get("/health", (_req, res) => {
  res.json({ status: "ok", sessions: Object.keys(transports).length });
});

app.listen(PORT, "0.0.0.0", () => {
  console.log(`HA MCP server listening on http://0.0.0.0:${PORT}/mcp`);
});
