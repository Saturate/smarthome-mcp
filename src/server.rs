use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use rmcp::tool_router;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpServerConfig,
    tower::StreamableHttpService,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::ha::HaClient;
use crate::z2m::Z2mClient;

#[derive(Clone)]
pub struct SmartHomeMcp {
    ha: Option<HaClient>,
    z2m: Option<Z2mClient>,
}

impl SmartHomeMcp {
    fn text_result(data: impl serde::Serialize) -> String {
        serde_json::to_string_pretty(&data).unwrap_or_else(|e| format!("serialization error: {e}"))
    }

    fn ha_or_err(&self) -> Result<&HaClient, String> {
        self.ha
            .as_ref()
            .ok_or_else(|| "Home Assistant backend is not configured".to_string())
    }

    fn z2m_or_err(&self) -> Result<&Z2mClient, String> {
        self.z2m
            .as_ref()
            .ok_or_else(|| "Zigbee2MQTT backend is not configured".to_string())
    }
}

// ── Tool parameter types ────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct EntityIdParam {
    #[schemars(description = "Entity id, e.g. light.kitchen or sensor.temperature")]
    entity_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AreaParam {
    #[schemars(description = "Area id as known in Home Assistant")]
    area: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EntityListParam {
    #[schemars(description = "Entity domain: light, sensor, switch, binary_sensor, climate, etc.")]
    domain: String,
    #[schemars(description = "Filter by device_class attribute, e.g. temperature, humidity, motion")]
    device_class: Option<String>,
    #[schemars(description = "Filter by state value, e.g. on, off. Omit to return all.")]
    state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EntitySearchParam {
    #[schemars(description = "Search term to match against entity_id and friendly_name")]
    query: String,
    #[schemars(description = "Optionally restrict to a domain (light, sensor, etc.)")]
    domain: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BrightnessParam {
    #[schemars(description = "The light entity id")]
    entity_id: String,
    #[schemars(description = "Brightness level 0-255")]
    brightness: u8,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TodoGetParam {
    #[schemars(description = "Todo list entity id, e.g. todo.shopping_list")]
    entity_id: String,
    #[schemars(description = "Filter by status: needs_action or completed. Omit for all.")]
    status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TodoAddParam {
    #[schemars(description = "Todo list entity id, e.g. todo.shopping_list")]
    entity_id: String,
    #[schemars(description = "The item text to add")]
    item: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TodoUpdateParam {
    #[schemars(description = "Todo list entity id")]
    entity_id: String,
    #[schemars(description = "Current item text (must match exactly)")]
    item: String,
    #[schemars(description = "New text for the item")]
    rename: Option<String>,
    #[schemars(description = "New status: needs_action or completed")]
    status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TodoRemoveParam {
    #[schemars(description = "Todo list entity id")]
    entity_id: String,
    #[schemars(description = "The item text to remove (must match exactly)")]
    item: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Z2mDeviceNameParam {
    #[schemars(description = "Zigbee device friendly name")]
    device: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Z2mDeviceSetParam {
    #[schemars(description = "Zigbee device friendly name")]
    device: String,
    #[schemars(description = "JSON payload with device attributes to set (e.g. {\"state\": \"ON\", \"brightness\": 200})")]
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Z2mDeviceRenameParam {
    #[schemars(description = "Current device friendly name")]
    from: String,
    #[schemars(description = "New device friendly name")]
    to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Z2mGroupAddParam {
    #[schemars(description = "Group friendly name")]
    friendly_name: String,
    #[schemars(description = "Optional group ID")]
    id: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Z2mPermitJoinParam {
    #[schemars(description = "Enable or disable permit join")]
    value: bool,
    #[schemars(description = "Optional: only permit join for a specific device")]
    device: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ServiceCallParam {
    #[schemars(description = "Service domain, e.g. switch, climate")]
    domain: String,
    #[schemars(description = "Service name, e.g. turn_on, set_temperature")]
    service: String,
    #[schemars(description = "Service data payload")]
    data: Option<serde_json::Value>,
}

// ── State summarization ─────────────────────────────────────────────

const USEFUL_ATTRIBUTES: &[&str] = &[
    "brightness",
    "color_temp",
    "rgb_color",
    "unit_of_measurement",
    "device_class",
    "current_temperature",
    "temperature",
    "hvac_action",
];

fn summarize_state(entity: &crate::ha::client::HaEntityState) -> serde_json::Value {
    let mut result = serde_json::json!({
        "entity_id": entity.entity_id,
        "state": entity.state,
        "friendly_name": entity.attributes.get("friendly_name")
            .unwrap_or(&serde_json::Value::String(entity.entity_id.clone())),
    });
    let obj = result.as_object_mut().unwrap();
    for key in USEFUL_ATTRIBUTES {
        if let Some(val) = entity.attributes.get(*key) {
            obj.insert((*key).to_string(), val.clone());
        }
    }
    result
}

// ── Tool implementations ────────────────────────────────────────────

#[tool_router(server_handler)]
impl SmartHomeMcp {
    // ── Entity state queries ────────────────────────────────────────

    #[tool(name = "ha_entity.get_state", description = "Get the current state and attributes of any entity")]
    async fn ha_entity_get_state(&self, Parameters(p): Parameters<EntityIdParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.get_state(&p.entity_id).await {
            Ok(state) => Self::text_result(summarize_state(&state)),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_entity.list", description = "List all entities for a domain (e.g. light, sensor, switch, climate) with their current states. Optionally filter by device_class or state.")]
    async fn ha_entity_list(&self, Parameters(p): Parameters<EntityListParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.get_states_by_domain(&p.domain).await {
            Ok(mut entities) => {
                if let Some(ref dc) = p.device_class {
                    entities.retain(|e| {
                        e.attributes
                            .get("device_class")
                            .and_then(|v| v.as_str())
                            == Some(dc)
                    });
                }
                if let Some(ref state) = p.state {
                    entities.retain(|e| e.state == *state);
                }
                let ids: Vec<String> = entities.iter().map(|e| e.entity_id.clone()).collect();
                let meta_map = ha.get_entity_meta_map(&ids).await.unwrap_or_default();
                let result: Vec<serde_json::Value> = entities
                    .iter()
                    .map(|e| {
                        let mut s = summarize_state(e);
                        let obj = s.as_object_mut().unwrap();
                        if let Some(meta) = meta_map.get(&e.entity_id) {
                            obj.insert("area".to_string(), serde_json::json!(meta.area));
                            obj.insert("device".to_string(), serde_json::json!(meta.device));
                        }
                        s
                    })
                    .collect();
                Self::text_result(result)
            }
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_entity.search", description = "Search entities by keyword across entity IDs and friendly names. Returns matching entities with their current states.")]
    async fn ha_entity_search(&self, Parameters(p): Parameters<EntitySearchParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        let states_result = match p.domain {
            Some(ref d) => ha.get_states_by_domain(d).await,
            None => ha.get_states().await,
        };
        match states_result {
            Ok(states) => {
                let q = p.query.to_lowercase();
                let matches: Vec<_> = states
                    .iter()
                    .filter(|e| {
                        let name = e
                            .attributes
                            .get("friendly_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        e.entity_id.to_lowercase().contains(&q) || name.contains(&q)
                    })
                    .collect();
                let ids: Vec<String> = matches.iter().map(|e| e.entity_id.clone()).collect();
                let meta_map = ha.get_entity_meta_map(&ids).await.unwrap_or_default();
                let result: Vec<serde_json::Value> = matches
                    .iter()
                    .map(|e| {
                        let mut s = summarize_state(e);
                        let obj = s.as_object_mut().unwrap();
                        if let Some(meta) = meta_map.get(&e.entity_id) {
                            obj.insert("area".to_string(), serde_json::json!(meta.area));
                            obj.insert("device".to_string(), serde_json::json!(meta.device));
                        }
                        s
                    })
                    .collect();
                Self::text_result(result)
            }
            Err(e) => format!("error: {e}"),
        }
    }

    // ── Area queries ────────────────────────────────────────────────

    #[tool(name = "ha_area.list", description = "List all areas configured in Home Assistant")]
    async fn ha_area_list(&self) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.get_areas().await {
            Ok(area_ids) => {
                let mut areas = Vec::new();
                for id in &area_ids {
                    let name = ha.get_area_name(id).await.unwrap_or_else(|_| id.clone());
                    areas.push(serde_json::json!({"id": id, "name": name}));
                }
                Self::text_result(areas)
            }
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_area.get_status", description = "Get the state of all entities in an area")]
    async fn ha_area_get_status(&self, Parameters(p): Parameters<AreaParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.get_entities_in_area(&p.area).await {
            Ok(entity_ids) => {
                let mut states = Vec::new();
                for id in &entity_ids {
                    match ha.get_state(id).await {
                        Ok(s) => states.push(summarize_state(&s)),
                        Err(e) => {
                            states.push(serde_json::json!({"entity_id": id, "error": e.to_string()}))
                        }
                    }
                }
                Self::text_result(serde_json::json!({"area": p.area, "entities": states}))
            }
            Err(e) => format!("error: {e}"),
        }
    }

    // ── Todo read ───────────────────────────────────────────────────

    #[tool(name = "ha_todo.get_items", description = "Get items from a Home Assistant todo list. Use ha_entity.list with domain 'todo' to discover available lists first.")]
    async fn ha_todo_get_items(&self, Parameters(p): Parameters<TodoGetParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        let mut data = serde_json::json!({"entity_id": p.entity_id});
        if let Some(status) = p.status {
            data["status"] = serde_json::Value::String(status);
        }
        match ha.call_service_with_response("todo", "get_items", data).await {
            Ok(result) => Self::text_result(result),
            Err(e) => format!("error: {e}"),
        }
    }

    // ── Light controls ──────────────────────────────────────────────

    #[tool(name = "ha_light.turn_on", description = "Turn on a light entity")]
    async fn ha_light_turn_on(&self, Parameters(p): Parameters<EntityIdParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.call_service("light", "turn_on", serde_json::json!({"entity_id": p.entity_id})).await {
            Ok(result) => Self::text_result(result.iter().map(summarize_state).collect::<Vec<_>>()),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_light.turn_off", description = "Turn off a light entity")]
    async fn ha_light_turn_off(&self, Parameters(p): Parameters<EntityIdParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.call_service("light", "turn_off", serde_json::json!({"entity_id": p.entity_id})).await {
            Ok(result) => Self::text_result(result.iter().map(summarize_state).collect::<Vec<_>>()),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_light.set_brightness", description = "Set the brightness of a light (0-255)")]
    async fn ha_light_set_brightness(&self, Parameters(p): Parameters<BrightnessParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.call_service("light", "turn_on", serde_json::json!({"entity_id": p.entity_id, "brightness": p.brightness})).await {
            Ok(result) => Self::text_result(result.iter().map(summarize_state).collect::<Vec<_>>()),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_light.turn_on_in_area", description = "Turn on all lights in a named area (e.g. kitchen, living_room)")]
    async fn ha_light_turn_on_in_area(&self, Parameters(p): Parameters<AreaParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.get_entities_in_area(&p.area).await {
            Ok(entities) => {
                let lights: Vec<_> = entities.into_iter().filter(|e| e.starts_with("light.")).collect();
                if lights.is_empty() {
                    return Self::text_result(serde_json::json!({"message": format!("No lights found in area '{}'", p.area)}));
                }
                match ha.call_service("light", "turn_on", serde_json::json!({"entity_id": lights})).await {
                    Ok(result) => Self::text_result(result.iter().map(summarize_state).collect::<Vec<_>>()),
                    Err(e) => format!("error: {e}"),
                }
            }
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_light.turn_off_in_area", description = "Turn off all lights in a named area")]
    async fn ha_light_turn_off_in_area(&self, Parameters(p): Parameters<AreaParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.get_entities_in_area(&p.area).await {
            Ok(entities) => {
                let lights: Vec<_> = entities.into_iter().filter(|e| e.starts_with("light.")).collect();
                if lights.is_empty() {
                    return Self::text_result(serde_json::json!({"message": format!("No lights found in area '{}'", p.area)}));
                }
                match ha.call_service("light", "turn_off", serde_json::json!({"entity_id": lights})).await {
                    Ok(result) => Self::text_result(result.iter().map(summarize_state).collect::<Vec<_>>()),
                    Err(e) => format!("error: {e}"),
                }
            }
            Err(e) => format!("error: {e}"),
        }
    }

    // ── Todo write ──────────────────────────────────────────────────

    #[tool(name = "ha_todo.add_item", description = "Add an item to a Home Assistant todo list")]
    async fn ha_todo_add_item(&self, Parameters(p): Parameters<TodoAddParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.call_service("todo", "add_item", serde_json::json!({"entity_id": p.entity_id, "item": p.item})).await {
            Ok(result) => Self::text_result(serde_json::json!({"added": p.item, "list": p.entity_id, "state": result})),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_todo.update_item", description = "Update a todo item's text or status (mark as completed/needs_action). Use ha_todo.get_items first to find the item name.")]
    async fn ha_todo_update_item(&self, Parameters(p): Parameters<TodoUpdateParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        let mut data = serde_json::json!({"entity_id": p.entity_id, "item": p.item});
        if let Some(rename) = &p.rename {
            data["rename"] = serde_json::Value::String(rename.clone());
        }
        if let Some(status) = &p.status {
            data["status"] = serde_json::Value::String(status.clone());
        }
        match ha.call_service("todo", "update_item", data).await {
            Ok(result) => Self::text_result(serde_json::json!({"updated": p.item, "list": p.entity_id, "state": result})),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "ha_todo.remove_item", description = "Remove an item from a Home Assistant todo list")]
    async fn ha_todo_remove_item(&self, Parameters(p): Parameters<TodoRemoveParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        match ha.call_service("todo", "remove_item", serde_json::json!({"entity_id": p.entity_id, "item": p.item})).await {
            Ok(result) => Self::text_result(serde_json::json!({"removed": p.item, "list": p.entity_id, "state": result})),
            Err(e) => format!("error: {e}"),
        }
    }

    // ── Generic service call ────────────────────────────────────────

    #[tool(name = "ha_service.call", description = "Call any Home Assistant service (escape hatch for anything not covered by other tools)")]
    async fn ha_service_call(&self, Parameters(p): Parameters<ServiceCallParam>) -> String {
        let ha = match self.ha_or_err() { Ok(h) => h, Err(e) => return e };
        let data = p.data.unwrap_or(serde_json::json!({}));
        match ha.call_service(&p.domain, &p.service, data).await {
            Ok(result) => Self::text_result(result),
            Err(e) => format!("error: {e}"),
        }
    }

    // ── Z2M read tools ──────────────────────────────────────────────

    #[tool(name = "z2m_device.list", description = "List all Zigbee devices with their exposed features")]
    async fn z2m_device_list(&self) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        let devices = z2m.get_devices().await;
        let result: Vec<serde_json::Value> = devices
            .iter()
            .map(|d| {
                let mut v = serde_json::json!({
                    "friendly_name": d.friendly_name,
                    "ieee_address": d.ieee_address,
                    "type": d.device_type,
                });
                if let Some(ref def) = d.definition {
                    v["model"] = serde_json::json!(def.model);
                    v["vendor"] = serde_json::json!(def.vendor);
                    v["description"] = serde_json::json!(def.description);
                    v["exposes"] = serde_json::json!(def.exposes);
                }
                v
            })
            .collect();
        Self::text_result(result)
    }

    #[tool(name = "z2m_device.get_state", description = "Get current state of a Zigbee device (includes availability)")]
    async fn z2m_device_get_state(&self, Parameters(p): Parameters<Z2mDeviceNameParam>) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        let state = z2m.get_device_state(&p.device).await;
        let availability = z2m.get_device_availability(&p.device).await;
        Self::text_result(serde_json::json!({
            "device": p.device,
            "state": state,
            "available": availability,
        }))
    }

    #[tool(name = "z2m_group.list", description = "List all Zigbee2MQTT groups")]
    async fn z2m_group_list(&self) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        Self::text_result(z2m.get_groups().await)
    }

    #[tool(name = "z2m_bridge.info", description = "Get Zigbee2MQTT bridge info (coordinator, version, network)")]
    async fn z2m_bridge_info(&self) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        match z2m.get_bridge_info().await {
            Some(info) => Self::text_result(info),
            None => Self::text_result(serde_json::json!({"message": "bridge info not yet available"})),
        }
    }

    // ── Z2M control tools ───────────────────────────────────────────

    #[tool(name = "z2m_device.set", description = "Set Zigbee device state. Payload is validated against the device's exposes definition.")]
    async fn z2m_device_set(&self, Parameters(p): Parameters<Z2mDeviceSetParam>) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };

        let devices = z2m.get_devices().await;
        if let Some(exposes) = z2m.find_device_exposes(&devices, &p.device)
            && let Err(errors) = z2m.validate_payload(&exposes, &p.payload)
        {
            return Self::text_result(serde_json::json!({
                "error": "payload validation failed",
                "errors": errors,
            }));
        }

        let availability = z2m.get_device_availability(&p.device).await;
        match z2m.set_device_state(&p.device, p.payload).await {
            Ok(()) => {
                let mut result = serde_json::json!({"device": p.device, "status": "sent"});
                if availability == Some(false) {
                    result["warning"] = serde_json::json!("device is currently unavailable; command queued");
                }
                Self::text_result(result)
            }
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "z2m_device.rename", description = "Rename a Zigbee device")]
    async fn z2m_device_rename(&self, Parameters(p): Parameters<Z2mDeviceRenameParam>) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        match z2m.bridge_request("device/rename", serde_json::json!({"from": p.from, "to": p.to})).await {
            Ok(resp) => Self::text_result(resp),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "z2m_group.add", description = "Create a Zigbee2MQTT group")]
    async fn z2m_group_add(&self, Parameters(p): Parameters<Z2mGroupAddParam>) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        let mut data = serde_json::json!({"friendly_name": p.friendly_name});
        if let Some(id) = p.id {
            data["id"] = serde_json::json!(id);
        }
        match z2m.bridge_request("group/add", data).await {
            Ok(resp) => Self::text_result(resp),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "z2m_bridge.permit_join", description = "Enable or disable Zigbee permit join")]
    async fn z2m_bridge_permit_join(&self, Parameters(p): Parameters<Z2mPermitJoinParam>) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        let mut data = serde_json::json!({"value": p.value});
        if let Some(device) = p.device {
            data["device"] = serde_json::json!(device);
        }
        match z2m.bridge_request("permit_join", data).await {
            Ok(resp) => Self::text_result(resp),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(name = "z2m_bridge.networkmap", description = "Request Zigbee network map")]
    async fn z2m_bridge_networkmap(&self) -> String {
        let z2m = match self.z2m_or_err() { Ok(z) => z, Err(e) => return e };
        match z2m.bridge_request("networkmap", serde_json::json!({"type": "raw", "routes": false})).await {
            Ok(resp) => Self::text_result(resp),
            Err(e) => format!("error: {e}"),
        }
    }
}

pub fn create_router() -> axum::Router {
    create_mcp_router(None, None)
}

pub fn create_router_with_ha(ha: Option<HaClient>) -> axum::Router {
    create_mcp_router(ha, None)
}

pub fn create_mcp_router(ha: Option<HaClient>, z2m: Option<Z2mClient>) -> axum::Router {
    let ct = CancellationToken::new();
    let config = StreamableHttpServerConfig::default().with_cancellation_token(ct);
    let session_manager = Arc::new(LocalSessionManager::default());

    let ha_for_health = ha.clone();
    let z2m_for_health = z2m.clone();

    let service = StreamableHttpService::new(
        move || {
            Ok(SmartHomeMcp {
                ha: ha.clone(),
                z2m: z2m.clone(),
            })
        },
        session_manager,
        config,
    );

    axum::Router::new()
        .route(
            "/health",
            axum::routing::get(move || health_handler(ha_for_health.clone(), z2m_for_health.clone())),
        )
        .nest_service("/mcp", service)
}

async fn health_handler(
    ha: Option<HaClient>,
    z2m: Option<Z2mClient>,
) -> axum::Json<serde_json::Value> {
    let mut backends = serde_json::Map::new();
    let mut all_ok = true;
    let mut any_configured = false;

    if let Some(ref ha) = ha {
        any_configured = true;
        match ha.check_connection().await {
            Ok(()) => {
                backends.insert(
                    "ha".to_string(),
                    serde_json::json!({"status": "ok"}),
                );
            }
            Err(e) => {
                all_ok = false;
                backends.insert(
                    "ha".to_string(),
                    serde_json::json!({"status": "unavailable", "error": e.to_string()}),
                );
            }
        }
    }

    if let Some(ref z2m) = z2m {
        any_configured = true;
        if z2m.is_connected().await {
            backends.insert(
                "z2m".to_string(),
                serde_json::json!({"status": "ok"}),
            );
        } else {
            all_ok = false;
            backends.insert(
                "z2m".to_string(),
                serde_json::json!({"status": "unavailable", "error": "MQTT not connected"}),
            );
        }
    }

    let status = if !any_configured || all_ok {
        "ok"
    } else if backends.values().all(|v| v.get("status").and_then(|s| s.as_str()) == Some("unavailable")) {
        "unavailable"
    } else {
        "degraded"
    };

    axum::Json(serde_json::json!({
        "status": status,
        "backends": backends,
    }))
}
