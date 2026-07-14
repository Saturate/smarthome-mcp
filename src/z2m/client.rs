use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, oneshot};

use crate::config::Z2mConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Z2mDevice {
    pub ieee_address: String,
    pub friendly_name: String,
    #[serde(rename = "type")]
    pub device_type: String,
    #[serde(default)]
    pub definition: Option<DeviceDefinition>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDefinition {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub exposes: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Z2mGroup {
    pub id: u32,
    pub friendly_name: String,
    #[serde(default)]
    pub members: Vec<GroupMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    pub ieee_address: String,
    pub endpoint: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub status: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Z2mClient {
    mqtt: AsyncClient,
    base_topic: String,
    state: Arc<Z2mState>,
}

struct Z2mState {
    devices: RwLock<Vec<Z2mDevice>>,
    groups: RwLock<Vec<Z2mGroup>>,
    bridge_info: RwLock<Option<Value>>,
    device_states: RwLock<HashMap<String, Value>>,
    availability: RwLock<HashMap<String, bool>>,
    connected: RwLock<bool>,
    bridge_responses: RwLock<HashMap<String, Vec<oneshot::Sender<BridgeResponse>>>>,
}

impl Z2mState {
    fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
            groups: RwLock::new(Vec::new()),
            bridge_info: RwLock::new(None),
            device_states: RwLock::new(HashMap::new()),
            availability: RwLock::new(HashMap::new()),
            connected: RwLock::new(false),
            bridge_responses: RwLock::new(HashMap::new()),
        }
    }
}

impl Z2mClient {
    pub async fn new(config: &Z2mConfig) -> Result<Self, Z2mError> {
        let base_topic = config.base_topic.clone();

        let url = &config.mqtt_host;
        let (host, port) = parse_mqtt_url(url)?;

        let client_id = format!("smarthome-mcp-{}", std::process::id());
        let mut opts = MqttOptions::new(client_id, &host, port);
        opts.set_keep_alive(Duration::from_secs(30));
        // Z2M bridge/devices can be large with many devices
        opts.set_max_packet_size(1024 * 1024, 1024 * 1024);

        if let (Some(user), Some(pass)) = (&config.mqtt_user, &config.mqtt_pass) {
            opts.set_credentials(user, pass);
        }

        let (mqtt, mut eventloop) = AsyncClient::new(opts, 100);
        let state = Arc::new(Z2mState::new());

        // Subscribe to bridge topics and device availability
        let topics = [
            format!("{base_topic}/bridge/devices"),
            format!("{base_topic}/bridge/groups"),
            format!("{base_topic}/bridge/info"),
            format!("{base_topic}/bridge/state"),
            format!("{base_topic}/bridge/response/#"),
            format!("{base_topic}/+/availability"),
        ];
        for topic in &topics {
            mqtt.subscribe(topic, QoS::AtMostOnce)
                .await
                .map_err(|e| Z2mError::Mqtt(format!("subscribe to {topic}: {e}")))?;
        }

        let event_state = state.clone();
        let event_base = base_topic.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(msg))) => {
                        Self::handle_message(&event_state, &event_base, &msg.topic, &msg.payload)
                            .await;
                    }
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        *event_state.connected.write().await = true;
                        tracing::info!("Z2M MQTT connected");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        *event_state.connected.write().await = false;
                        tracing::warn!(error = %e, "Z2M MQTT connection error, retrying...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Self {
            mqtt,
            base_topic,
            state,
        })
    }

    async fn handle_message(state: &Z2mState, base_topic: &str, topic: &str, payload: &[u8]) {
        if topic == format!("{base_topic}/bridge/devices") {
            if let Ok(devices) = serde_json::from_slice::<Vec<Z2mDevice>>(payload) {
                // Subscribe to device state topics
                tracing::info!(count = devices.len(), "Z2M device list updated");
                *state.devices.write().await = devices;
            }
        } else if topic == format!("{base_topic}/bridge/groups") {
            if let Ok(groups) = serde_json::from_slice::<Vec<Z2mGroup>>(payload) {
                *state.groups.write().await = groups;
            }
        } else if topic == format!("{base_topic}/bridge/info") {
            if let Ok(info) = serde_json::from_slice::<Value>(payload) {
                *state.bridge_info.write().await = Some(info);
            }
        } else if let Some(rest) = topic.strip_prefix(&format!("{base_topic}/bridge/response/")) {
            if let Ok(resp) = serde_json::from_slice::<BridgeResponse>(payload) {
                let mut responses = state.bridge_responses.write().await;
                if let Some(senders) = responses.remove(rest) {
                    for sender in senders {
                        let _ = sender.send(resp.clone());
                    }
                }
            }
        } else if let Some(device_name) = topic
            .strip_prefix(&format!("{base_topic}/"))
            .and_then(|rest| rest.strip_suffix("/availability"))
        {
            if let Ok(avail) = serde_json::from_slice::<Value>(payload) {
                let online = avail
                    .get("state")
                    .and_then(|s| s.as_str())
                    .map(|s| s == "online")
                    .unwrap_or(false);
                state
                    .availability
                    .write()
                    .await
                    .insert(device_name.to_string(), online);
            }
        } else if let Some(device_name) = topic.strip_prefix(&format!("{base_topic}/"))
            && !device_name.contains('/')
            && let Ok(state_val) = serde_json::from_slice::<Value>(payload)
        {
            state
                .device_states
                .write()
                .await
                .insert(device_name.to_string(), state_val);
        }
    }

    pub async fn is_connected(&self) -> bool {
        *self.state.connected.read().await
    }

    pub async fn get_devices(&self) -> Vec<Z2mDevice> {
        self.state.devices.read().await.clone()
    }

    pub async fn get_device_state(&self, device_name: &str) -> Option<Value> {
        // First try cached state
        if let Some(state) = self.state.device_states.read().await.get(device_name) {
            return Some(state.clone());
        }

        // Subscribe and request state
        let topic = format!("{}/{device_name}", self.base_topic);
        let _ = self.mqtt.subscribe(&topic, QoS::AtMostOnce).await;
        let get_topic = format!("{}/{device_name}/get", self.base_topic);
        let _ = self
            .mqtt
            .publish(&get_topic, QoS::AtLeastOnce, false, r#"{"state": ""}"#)
            .await;

        // Wait briefly for response
        tokio::time::sleep(Duration::from_millis(500)).await;
        self.state
            .device_states
            .read()
            .await
            .get(device_name)
            .cloned()
    }

    pub async fn get_device_availability(&self, device_name: &str) -> Option<bool> {
        self.state
            .availability
            .read()
            .await
            .get(device_name)
            .copied()
    }

    pub async fn set_device_state(
        &self,
        device_name: &str,
        payload: Value,
    ) -> Result<(), Z2mError> {
        let topic = format!("{}/{device_name}/set", self.base_topic);
        let data = serde_json::to_vec(&payload)
            .map_err(|e| Z2mError::Parse(format!("serialize payload: {e}")))?;
        self.mqtt
            .publish(&topic, QoS::AtLeastOnce, false, data)
            .await
            .map_err(|e| Z2mError::Mqtt(format!("publish to {topic}: {e}")))?;
        Ok(())
    }

    pub async fn bridge_request(
        &self,
        command: &str,
        data: Value,
    ) -> Result<BridgeResponse, Z2mError> {
        let (tx, rx) = oneshot::channel();

        {
            let mut responses = self.state.bridge_responses.write().await;
            responses.entry(command.to_string()).or_default().push(tx);
        }

        let topic = format!("{}/bridge/request/{command}", self.base_topic);
        let payload = serde_json::to_vec(&data)
            .map_err(|e| Z2mError::Parse(format!("serialize request: {e}")))?;
        self.mqtt
            .publish(&topic, QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| Z2mError::Mqtt(format!("publish bridge request: {e}")))?;

        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => Err(Z2mError::Mqtt("bridge response channel closed".to_string())),
            Err(_) => Err(Z2mError::Timeout),
        }
    }

    pub async fn get_groups(&self) -> Vec<Z2mGroup> {
        self.state.groups.read().await.clone()
    }

    pub async fn get_bridge_info(&self) -> Option<Value> {
        self.state.bridge_info.read().await.clone()
    }

    pub fn find_device_exposes(
        &self,
        devices: &[Z2mDevice],
        device_name: &str,
    ) -> Option<Vec<Value>> {
        devices
            .iter()
            .find(|d| d.friendly_name == device_name)
            .and_then(|d| d.definition.as_ref())
            .map(|def| def.exposes.clone())
    }

    pub fn validate_payload(&self, exposes: &[Value], payload: &Value) -> Result<(), Vec<String>> {
        let Some(obj) = payload.as_object() else {
            return Err(vec!["payload must be a JSON object".to_string()]);
        };

        let valid_fields = collect_exposed_fields(exposes);
        let mut errors = Vec::new();

        for key in obj.keys() {
            if !valid_fields.contains_key(key.as_str()) {
                let available: Vec<&str> = valid_fields.keys().copied().collect();
                errors.push(format!(
                    "unknown field '{key}'. Available: {}",
                    available.join(", ")
                ));
            }
        }

        for (key, value) in obj {
            if let Some(field_def) = valid_fields.get(key.as_str()) {
                if let Some(min) = field_def.get("value_min").and_then(|v| v.as_f64())
                    && let Some(val) = value.as_f64()
                    && val < min
                {
                    errors.push(format!("'{key}' must be >= {min}, got {val}"));
                }
                if let Some(max) = field_def.get("value_max").and_then(|v| v.as_f64())
                    && let Some(val) = value.as_f64()
                    && val > max
                {
                    errors.push(format!("'{key}' must be <= {max}, got {val}"));
                }
                if let Some(values) = field_def.get("values").and_then(|v| v.as_array())
                    && let Some(val_str) = value.as_str()
                {
                    let allowed: Vec<&str> = values.iter().filter_map(|v| v.as_str()).collect();
                    if !allowed.contains(&val_str) {
                        errors.push(format!(
                            "'{key}' must be one of [{}], got '{val_str}'",
                            allowed.join(", ")
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn collect_exposed_fields(exposes: &[Value]) -> HashMap<&str, &Value> {
    let mut fields = HashMap::new();
    for expose in exposes {
        if let Some(features) = expose.get("features").and_then(|f| f.as_array()) {
            for feature in features {
                if let Some(name) = feature.get("name").and_then(|n| n.as_str()) {
                    fields.insert(name, feature);
                }
            }
        }
        if let Some(name) = expose.get("name").and_then(|n| n.as_str()) {
            fields.insert(name, expose);
        }
    }
    fields
}

fn parse_mqtt_url(url: &str) -> Result<(String, u16), Z2mError> {
    let url = url
        .strip_prefix("mqtt://")
        .or_else(|| url.strip_prefix("tcp://"))
        .unwrap_or(url);

    let (host, port) = if let Some((h, p)) = url.rsplit_once(':') {
        let port: u16 = p
            .parse()
            .map_err(|_| Z2mError::Parse(format!("invalid MQTT port: {p}")))?;
        (h.to_string(), port)
    } else {
        (url.to_string(), 1883)
    };

    Ok((host, port))
}

#[derive(Debug, thiserror::Error)]
pub enum Z2mError {
    #[error("MQTT error: {0}")]
    Mqtt(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("request timed out")]
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mqtt_url_with_scheme() {
        let (host, port) = parse_mqtt_url("mqtt://localhost:1883").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 1883);
    }

    #[test]
    fn parse_mqtt_url_without_scheme() {
        let (host, port) = parse_mqtt_url("192.168.1.100:1884").unwrap();
        assert_eq!(host, "192.168.1.100");
        assert_eq!(port, 1884);
    }

    #[test]
    fn parse_mqtt_url_default_port() {
        let (host, port) = parse_mqtt_url("mqtt://broker.local").unwrap();
        assert_eq!(host, "broker.local");
        assert_eq!(port, 1883);
    }

    #[test]
    fn validate_payload_valid() {
        let exposes = vec![serde_json::json!({
            "type": "light",
            "features": [
                {"name": "state", "type": "binary", "values": ["ON", "OFF"]},
                {"name": "brightness", "type": "numeric", "value_min": 0, "value_max": 254}
            ]
        })];
        let client = make_test_client();
        let payload = serde_json::json!({"state": "ON", "brightness": 200});
        assert!(client.validate_payload(&exposes, &payload).is_ok());
    }

    #[test]
    fn validate_payload_unknown_field() {
        let exposes = vec![serde_json::json!({
            "type": "light",
            "features": [
                {"name": "state", "type": "binary", "values": ["ON", "OFF"]}
            ]
        })];
        let client = make_test_client();
        let payload = serde_json::json!({"bogus": "value"});
        let errors = client.validate_payload(&exposes, &payload).unwrap_err();
        assert!(errors[0].contains("unknown field 'bogus'"));
    }

    #[test]
    fn validate_payload_out_of_range() {
        let exposes = vec![serde_json::json!({
            "type": "light",
            "features": [
                {"name": "brightness", "type": "numeric", "value_min": 0, "value_max": 254}
            ]
        })];
        let client = make_test_client();
        let payload = serde_json::json!({"brightness": 300});
        let errors = client.validate_payload(&exposes, &payload).unwrap_err();
        assert!(errors[0].contains("must be <= 254"));
    }

    #[test]
    fn validate_payload_invalid_enum() {
        let exposes = vec![serde_json::json!({
            "type": "light",
            "features": [
                {"name": "state", "type": "binary", "values": ["ON", "OFF"]}
            ]
        })];
        let client = make_test_client();
        let payload = serde_json::json!({"state": "MAYBE"});
        let errors = client.validate_payload(&exposes, &payload).unwrap_err();
        assert!(errors[0].contains("must be one of"));
    }

    fn make_test_client() -> Z2mClient {
        let opts = MqttOptions::new("test", "localhost", 1883);
        let (mqtt, _) = AsyncClient::new(opts, 10);
        Z2mClient {
            mqtt,
            base_topic: "zigbee2mqtt".to_string(),
            state: Arc::new(Z2mState::new()),
        }
    }
}
