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

#[derive(Clone)]
pub struct SmartHomeMcp {
    ha: Option<HaClient>,
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
}

pub fn create_router() -> axum::Router {
    create_router_with_ha(None)
}

pub fn create_router_with_ha(ha: Option<HaClient>) -> axum::Router {
    let ct = CancellationToken::new();
    let config = StreamableHttpServerConfig::default().with_cancellation_token(ct);
    let session_manager = Arc::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(
        move || Ok(SmartHomeMcp { ha: ha.clone() }),
        session_manager,
        config,
    );
    axum::Router::new().nest_service("/mcp", service)
}
