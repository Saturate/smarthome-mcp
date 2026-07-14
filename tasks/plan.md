# Plan: smarthome-mcp Rust rewrite

All work on branch `rust-rewrite` off `main`.

---

## Task 1: Project scaffold + MCP hello world

**Description:** Create the Rust project with all dependencies in Cargo.toml, set up the rmcp streamable HTTP transport via axum, and register a single dummy `ping` tool. This validates that rmcp works end-to-end before building anything real. Create the branch.

**Acceptance criteria:**
- [ ] `rust-rewrite` branch created off `main`
- [ ] `Cargo.toml` with all dependencies from spec (rmcp, axum, reqwest, rumqttc, serde, schemars, toml, tracing, tokio)
- [ ] `src/main.rs` starts an axum server on `PORT` with rmcp `StreamableHttpService`
- [ ] One dummy tool (`ping`) responds through MCP streamable HTTP
- [ ] `cargo build` and `cargo clippy -- -D warnings` pass

**Verification:**
- [ ] `cargo build` succeeds
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: `curl` or MCP client connects to `/mcp` and calls `ping`

**Dependencies:** None

**Files likely touched:**
- `Cargo.toml`
- `src/main.rs`

**Estimated scope:** Small

---

## Task 2: Config module

**Description:** Build the config layer that parses env vars and optional TOML file. Config structs for server, HA backend, Z2M backend, and auth (whitelist rules, static tokens, proxy settings with scopes). Env vars override TOML for connection settings. Create `config.example.toml`.

**Acceptance criteria:**
- [ ] `src/config.rs` with typed structs for all config sections
- [ ] Loads TOML from `CONFIG_FILE` env var path when set
- [ ] Env vars (`HA_URL`, `HA_TOKEN`, `Z2M_MQTT_HOST`, etc.) override TOML values
- [ ] Either backend section can be omitted (both optional)
- [ ] Auth section optional (no auth = open mode)
- [ ] `config.example.toml` matches spec format
- [ ] Unit tests for: TOML parsing, env override, missing optional sections

**Verification:**
- [ ] `cargo test` passes config tests
- [ ] `cargo clippy -- -D warnings` clean

**Dependencies:** Task 1

**Files likely touched:**
- `src/config.rs`
- `src/main.rs` (load config)
- `config.example.toml`

**Estimated scope:** Medium

---

## Task 3: HA client + read tools

**Description:** Port the HA REST client from TypeScript and wire up all read-only HA tools through the MCP server. This is the first real vertical slice: config loads HA settings, client connects, tools respond to MCP calls.

**Acceptance criteria:**
- [ ] `src/ha/client.rs`: reqwest client with `get_states`, `get_state`, `get_states_by_domain`, `render_template`, `get_areas`, `get_area_name`, `get_entities_in_area`, `get_entity_meta_map`
- [ ] `src/util.rs`: `parse_jinja_list` ported from TS
- [ ] `src/ha/tools.rs`: tools registered: `ha_entity.get_state`, `ha_entity.list`, `ha_entity.search`, `ha_area.list`, `ha_area.get_status`, `ha_todo.get_items`
- [ ] State summarization matches TS version (entity_id, state, friendly_name, useful attributes)
- [ ] 10s timeout on all HA HTTP calls
- [ ] Unit tests for `parse_jinja_list` and client response parsing

**Verification:**
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: connect MCP client, call `ha_entity.list` with `domain: "light"` against a real HA

**Dependencies:** Task 2

**Files likely touched:**
- `src/ha/mod.rs`
- `src/ha/client.rs`
- `src/ha/tools.rs`
- `src/util.rs`
- `src/main.rs` (wire up HA tools)

**Estimated scope:** Medium

---

## Task 4: HA control tools

**Description:** Add all write/control HA tools: light controls, todo CRUD, and the generic service call escape hatch.

**Acceptance criteria:**
- [ ] Tools registered: `ha_light.turn_on`, `ha_light.turn_off`, `ha_light.set_brightness`, `ha_light.turn_on_in_area`, `ha_light.turn_off_in_area`, `ha_todo.add_item`, `ha_todo.update_item`, `ha_todo.remove_item`, `ha_service.call`
- [ ] `call_service` and `call_service_with_response` methods on HA client
- [ ] Area-based light tools resolve area entities then filter to `light.*`
- [ ] All 15 HA tools now registered (read + control)

**Verification:**
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: turn a light on/off via MCP tool call against real HA

**Dependencies:** Task 3

**Files likely touched:**
- `src/ha/client.rs` (add service call methods)
- `src/ha/tools.rs` (add control tools)

**Estimated scope:** Medium

---

## Checkpoint: After Tasks 1-4
- [ ] All tests pass (`cargo test`)
- [ ] `cargo clippy -- -D warnings` clean
- [ ] All 15 HA tools work identically to the TypeScript version
- [ ] MCP client can connect and call tools via streamable HTTP
- [ ] Review with human before proceeding

---

## Task 5: Auth scopes + IP whitelist

**Description:** Implement the scope system and IP whitelist auth strategy. Define scope types, map each tool to its required scope, and build axum middleware that checks client IP against configured CIDR ranges.

**Acceptance criteria:**
- [ ] `src/auth/scopes.rs`: `Scope` enum (`HaRead`, `HaControl`, `Z2mRead`, `Z2mControl`, `All`), `tool_required_scope()` function mapping tool names to scopes
- [ ] `src/auth/ip_whitelist.rs`: CIDR range parsing and IP matching
- [ ] `src/auth/mod.rs`: axum middleware that resolves client IP, evaluates whitelist, attaches granted scopes to request extensions
- [ ] Tool calls check granted scopes; missing scope returns JSON-RPC error `-32001`
- [ ] No auth config = open mode (all scopes granted)
- [ ] Unit tests for: CIDR matching, scope resolution, middleware with mock requests

**Verification:**
- [ ] `cargo test` passes auth tests
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: request from whitelisted IP succeeds, non-whitelisted IP without token gets rejected

**Dependencies:** Task 2 (config), Task 3 (tool names exist)

**Files likely touched:**
- `src/auth/mod.rs`
- `src/auth/scopes.rs`
- `src/auth/ip_whitelist.rs`
- `src/main.rs` (add middleware to router)

**Estimated scope:** Medium

---

## Task 6: Auth static tokens + proxy validation

**Description:** Add static token matching and proxy auth (validate tokens against HA REST API or MQTT broker). Includes token caching with TTL for proxy auth.

**Acceptance criteria:**
- [ ] `src/auth/token_static.rs`: match `Authorization: Bearer <token>` against `[[auth.tokens]]` entries, return associated scopes
- [ ] `src/auth/token_proxy.rs`: validate bearer token by calling HA `/api/` (check 200) or attempting MQTT connect; cache valid tokens with configurable TTL
- [ ] Auth middleware evaluates strategies in order: whitelist -> static token -> proxy (first match wins)
- [ ] Unit tests for: token matching, proxy validation (mocked HTTP/MQTT), cache expiry

**Verification:**
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: static token with `ha:read` scope can list entities but not turn on lights

**Dependencies:** Task 5

**Files likely touched:**
- `src/auth/token_static.rs`
- `src/auth/token_proxy.rs`
- `src/auth/mod.rs` (chain strategies)

**Estimated scope:** Medium

---

## Checkpoint: After Tasks 5-6
- [ ] All tests pass
- [ ] Auth layer fully functional with all three strategies
- [ ] Scoped access works: read-only token can't call control tools
- [ ] Open mode (no auth config) still works
- [ ] Review with human before proceeding

---

## Task 7: Z2M MQTT client + device cache

**Description:** Build the Zigbee2MQTT client using rumqttc. Connect to the MQTT broker, subscribe to bridge topics, and maintain an in-memory device cache with exposes definitions.

**Acceptance criteria:**
- [ ] `src/z2m/client.rs`: async MQTT client that connects to configured broker
- [ ] Subscribes to `{base_topic}/bridge/devices`, `{base_topic}/bridge/groups`, `{base_topic}/bridge/info`, `{base_topic}/bridge/state`
- [ ] Parses retained `bridge/devices` payload into typed structs including `exposes` definitions
- [ ] Device cache updates on new `bridge/devices` messages
- [ ] Subscribes to `{base_topic}/+/availability` for device availability tracking
- [ ] QoS 0 for subscriptions
- [ ] Methods: `get_devices()`, `get_device_state(name)`, `get_groups()`, `get_bridge_info()`
- [ ] Unit tests with mock MQTT messages

**Verification:**
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: connect to a real Z2M MQTT broker, verify device list populates

**Dependencies:** Task 2 (config)

**Files likely touched:**
- `src/z2m/mod.rs`
- `src/z2m/client.rs`

**Estimated scope:** Large (MQTT async + cache + exposes parsing; split not worth it since the parts are tightly coupled)

---

## Task 8: Z2M read tools

**Description:** Wire up read-only Z2M MCP tools using the Z2M client.

**Acceptance criteria:**
- [ ] Tools registered: `z2m_device.list` (includes exposes per device), `z2m_device.get_state` (includes availability), `z2m_group.list`, `z2m_bridge.info`
- [ ] Device list includes exposed features so MCP clients know what each device accepts
- [ ] State response includes availability status

**Verification:**
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: call `z2m_device.list` via MCP, see devices with exposes

**Dependencies:** Task 7

**Files likely touched:**
- `src/z2m/tools.rs`
- `src/main.rs` (register Z2M tools)

**Estimated scope:** Small

---

## Task 9: Z2M control tools

**Description:** Add Z2M control tools with exposes-based payload validation and bridge request/response commands.

**Acceptance criteria:**
- [ ] `z2m_device.set`: accepts device name + arbitrary JSON payload, validates against device's `exposes` definition (reject unknown fields, out-of-range values with helpful error), publishes to `{base_topic}/{device}/set` with QoS 1, includes availability warning if device is unavailable
- [ ] `z2m_device.rename`: bridge request/response pattern
- [ ] `z2m_group.add`: bridge request/response pattern
- [ ] `z2m_bridge.permit_join`: bridge request/response pattern
- [ ] `z2m_bridge.networkmap`: bridge request/response pattern
- [ ] Z2M client methods: `set_device_state(name, payload)`, `bridge_request(cmd, data)`
- [ ] Unit tests for exposes validation logic

**Verification:**
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Manual: turn a Zigbee light on/off via `z2m_device.set`

**Dependencies:** Task 7, Task 8

**Files likely touched:**
- `src/z2m/client.rs` (add publish/bridge_request methods)
- `src/z2m/tools.rs` (add control tools)

**Estimated scope:** Medium

---

## Checkpoint: After Tasks 7-9
- [ ] All tests pass
- [ ] All 9 Z2M tools work against a real Zigbee2MQTT instance
- [ ] Exposes validation rejects bad payloads with helpful errors
- [ ] Auth scopes apply to Z2M tools too
- [ ] Review with human before proceeding

---

## Task 10: Health endpoint + graceful degradation

**Description:** Add `/health` endpoint reporting per-backend status. Server starts even if backends are unreachable. Background reconnection with exponential backoff. Tools for unavailable backends return clear errors.

**Acceptance criteria:**
- [ ] `GET /health` returns JSON with top-level `status` (`ok` / `degraded` / `unavailable`), per-backend status, and session count
- [ ] Server starts successfully even if HA and/or Z2M are unreachable
- [ ] HA client: initial connection check on startup; background retry on failure
- [ ] Z2M client: MQTT reconnection with exponential backoff (rumqttc handles this natively)
- [ ] Tools for unavailable backend return JSON-RPC error `-32002` with message naming the backend
- [ ] `tools/list` still includes unavailable tools with a note in description
- [ ] When backend recovers, tools work again without restart

**Verification:**
- [ ] `cargo test` passes
- [ ] Manual: start server with HA_URL pointing to unreachable host, verify `/health` shows degraded, HA tools return `-32002`, Z2M tools work. Start HA, verify recovery.

**Dependencies:** Tasks 3-4 (HA tools), Tasks 7-9 (Z2M tools)

**Files likely touched:**
- `src/main.rs` (health endpoint, startup logic)
- `src/ha/client.rs` (availability check, reconnect)
- `src/z2m/client.rs` (availability tracking)

**Estimated scope:** Medium

---

## Task 11: Docker + CI

**Description:** Multi-arch Dockerfile using musl static builds targeting scratch. GitHub Actions CI (clippy, fmt, test) and release (cross-build + GHCR push) workflows.

**Acceptance criteria:**
- [ ] `Dockerfile`: multi-stage build, musl target, `scratch` runtime image
- [ ] `docker buildx build --platform linux/amd64,linux/arm64` succeeds
- [ ] Image size under 15MB per arch
- [ ] `.github/workflows/ci.yml`: runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` on push/PR
- [ ] `.github/workflows/release.yml`: cross-builds both arches, pushes multi-arch manifest to GHCR on version tags
- [ ] Old TS-specific CI files removed or replaced

**Verification:**
- [ ] `docker buildx build` succeeds locally
- [ ] `docker images` shows image under 15MB
- [ ] CI workflow passes on push to `rust-rewrite` branch

**Dependencies:** All previous tasks (needs complete codebase)

**Files likely touched:**
- `Dockerfile`
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`

**Estimated scope:** Medium

---

## Checkpoint: Final
- [ ] All 24 tools (15 HA + 9 Z2M) work via MCP streamable HTTP
- [ ] Auth scopes enforced across all strategies
- [ ] Graceful degradation works for both backends
- [ ] Docker image under 15MB
- [ ] CI green
- [ ] Ready for README + GitHub updates (separate task after merge)
