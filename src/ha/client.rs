use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::HaConfig;
use crate::util::parse_jinja_list;

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaEntityState {
    pub entity_id: String,
    pub state: String,
    pub attributes: HashMap<String, Value>,
    pub last_changed: String,
    pub last_updated: String,
}

#[derive(Debug, Clone)]
pub struct HaClient {
    client: Client,
    base_url: String,
}

impl HaClient {
    pub fn new(config: &HaConfig) -> Self {
        let base_url = config.url.trim_end_matches('/').to_string();
        let client = Client::builder()
            .timeout(TIMEOUT)
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", config.token).parse().unwrap(),
                );
                headers
            })
            .build()
            .expect("failed to build HTTP client");

        Self { client, base_url }
    }

    pub async fn get_states(&self) -> Result<Vec<HaEntityState>, HaError> {
        let resp = self
            .client
            .get(format!("{}/api/states", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn get_states_by_domain(&self, domain: &str) -> Result<Vec<HaEntityState>, HaError> {
        let states = self.get_states().await?;
        let prefix = format!("{domain}.");
        Ok(states
            .into_iter()
            .filter(|s| s.entity_id.starts_with(&prefix))
            .collect())
    }

    pub async fn get_state(&self, entity_id: &str) -> Result<HaEntityState, HaError> {
        let resp = self
            .client
            .get(format!("{}/api/states/{entity_id}", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn call_service(
        &self,
        domain: &str,
        service: &str,
        data: Value,
    ) -> Result<Vec<HaEntityState>, HaError> {
        let resp = self
            .client
            .post(format!("{}/api/services/{domain}/{service}", self.base_url))
            .json(&data)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn call_service_with_response(
        &self,
        domain: &str,
        service: &str,
        data: Value,
    ) -> Result<Value, HaError> {
        let resp = self
            .client
            .post(format!(
                "{}/api/services/{domain}/{service}?return_response",
                self.base_url
            ))
            .json(&data)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn render_template(&self, template: &str) -> Result<String, HaError> {
        let resp = self
            .client
            .post(format!("{}/api/template", self.base_url))
            .json(&serde_json::json!({ "template": template }))
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.text().await?)
    }

    pub async fn get_areas(&self) -> Result<Vec<String>, HaError> {
        let raw = self.render_template("{{ areas() | list }}").await?;
        Ok(parse_jinja_list(&raw))
    }

    pub async fn get_area_name(&self, area_id: &str) -> Result<String, HaError> {
        self.render_template(&format!("{{{{ area_name('{area_id}') }}}}"))
            .await
    }

    pub async fn get_entities_in_area(&self, area_id: &str) -> Result<Vec<String>, HaError> {
        let raw = self
            .render_template(&format!("{{{{ area_entities('{area_id}') | list }}}}"))
            .await?;
        Ok(parse_jinja_list(&raw))
    }

    pub async fn get_entity_meta_map(
        &self,
        entity_ids: &[String],
    ) -> Result<HashMap<String, EntityMeta>, HaError> {
        if entity_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_list = entity_ids
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let template = format!(
            r#"{{%- set ns = namespace(d={{}}) -%}}
{{%- for eid in [{id_list}] -%}}
  {{%- set ea = area_name(eid) -%}}
  {{%- set da = device_attr(eid, 'area_id') -%}}
  {{%- set dn = device_attr(eid, 'name_by_user') or device_attr(eid, 'name') -%}}
  {{%- set area = ea if ea else (area_name(da) if da else None) -%}}
  {{%- set ns.d = dict(ns.d, **{{eid: {{'area': area, 'device': dn}}}}) -%}}
{{%- endfor -%}}
{{{{ ns.d | tojson }}}}"#
        );
        let raw = self.render_template(&template).await?;
        serde_json::from_str(&raw).map_err(|e| HaError::Parse(e.to_string()))
    }

    pub async fn check_connection(&self) -> Result<(), HaError> {
        self.client
            .get(format!("{}/api/", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMeta {
    pub area: Option<String>,
    pub device: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HaError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("parse error: {0}")]
    Parse(String),
}
