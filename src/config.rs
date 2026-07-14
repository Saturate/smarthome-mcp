use std::net::IpAddr;
use std::path::Path;

use ipnet::IpNet;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub ha: Option<HaConfig>,
    pub z2m: Option<Z2mConfig>,
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { port: 3000 }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HaConfig {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Z2mConfig {
    pub mqtt_host: String,
    #[serde(default)]
    pub mqtt_user: Option<String>,
    #[serde(default)]
    pub mqtt_pass: Option<String>,
    #[serde(default = "default_base_topic")]
    pub base_topic: String,
}

fn default_base_topic() -> String {
    "zigbee2mqtt".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AuthConfig {
    pub whitelist: Vec<WhitelistEntry>,
    pub tokens: Vec<TokenEntry>,
    pub proxy: Option<ProxyAuthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhitelistEntry {
    pub cidrs: Vec<IpNet>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenEntry {
    pub token: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyAuthConfig {
    pub backend: ProxyBackend,
    #[serde(default = "default_all_scopes")]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub cache_ttl: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyBackend {
    Ha,
    Mqtt,
}

fn default_all_scopes() -> Vec<String> {
    vec!["*".to_string()]
}

impl Config {
    pub fn load() -> Self {
        let mut config = match std::env::var("CONFIG_FILE") {
            Ok(path) => Self::from_toml_file(&path),
            Err(_) => {
                if Path::new("config.toml").exists() {
                    Self::from_toml_file("config.toml")
                } else {
                    Config::default()
                }
            }
        };

        config.apply_env_overrides();
        config
    }

    fn from_toml_file(path: &str) -> Self {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read config file {path}: {e}"));
        toml::from_str(&content)
            .unwrap_or_else(|e| panic!("failed to parse config file {path}: {e}"))
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(port) = std::env::var("PORT")
            && let Ok(p) = port.parse()
        {
            self.server.port = p;
        }

        if let (Ok(url), Ok(token)) = (std::env::var("HA_URL"), std::env::var("HA_TOKEN")) {
            match &mut self.ha {
                Some(ha) => {
                    ha.url = url;
                    ha.token = token;
                }
                None => {
                    self.ha = Some(HaConfig { url, token });
                }
            }
        }

        if let Ok(host) = std::env::var("Z2M_MQTT_HOST") {
            let z2m = self.z2m.get_or_insert_with(|| Z2mConfig {
                mqtt_host: host.clone(),
                mqtt_user: None,
                mqtt_pass: None,
                base_topic: default_base_topic(),
            });
            z2m.mqtt_host = host;

            if let Ok(user) = std::env::var("Z2M_MQTT_USER") {
                z2m.mqtt_user = Some(user);
            }
            if let Ok(pass) = std::env::var("Z2M_MQTT_PASS") {
                z2m.mqtt_pass = Some(pass);
            }
            if let Ok(topic) = std::env::var("Z2M_BASE_TOPIC") {
                z2m.base_topic = topic;
            }
        }
    }

    pub fn is_open_auth(&self) -> bool {
        self.auth.is_none()
    }

    pub fn matches_whitelist(&self, ip: IpAddr) -> Option<&[String]> {
        let auth = self.auth.as_ref()?;
        for entry in &auth.whitelist {
            if entry.cidrs.iter().any(|cidr| cidr.contains(&ip)) {
                return Some(&entry.scopes);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"
            [server]
            port = 8080
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.port, 8080);
        assert!(config.ha.is_none());
        assert!(config.z2m.is_none());
        assert!(config.auth.is_none());
    }

    #[test]
    fn parse_full_toml() {
        let toml_str = r#"
            [server]
            port = 3001

            [ha]
            url = "http://ha.local:8123"
            token = "test-token"

            [z2m]
            mqtt_host = "mqtt://localhost:1883"
            base_topic = "z2m"

            [auth]
            [[auth.whitelist]]
            cidrs = ["192.168.0.0/16"]
            scopes = ["*"]

            [[auth.tokens]]
            token = "my-token"
            scopes = ["ha:read", "z2m:read"]

            [auth.proxy]
            backend = "ha"
            scopes = ["*"]
            cache_ttl = 300
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.port, 3001);

        let ha = config.ha.unwrap();
        assert_eq!(ha.url, "http://ha.local:8123");

        let z2m = config.z2m.unwrap();
        assert_eq!(z2m.base_topic, "z2m");

        let auth = config.auth.unwrap();
        assert_eq!(auth.whitelist.len(), 1);
        assert_eq!(auth.tokens.len(), 1);
        assert_eq!(auth.tokens[0].scopes, vec!["ha:read", "z2m:read"]);

        let proxy = auth.proxy.unwrap();
        assert_eq!(proxy.backend, ProxyBackend::Ha);
        assert_eq!(proxy.cache_ttl, 300);
    }

    #[test]
    fn parse_empty_toml() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.server.port, 3000);
        assert!(config.is_open_auth());
    }

    #[test]
    fn whitelist_matching() {
        let toml_str = r#"
            [auth]
            [[auth.whitelist]]
            cidrs = ["192.168.1.0/24"]
            scopes = ["ha:read"]

            [[auth.whitelist]]
            cidrs = ["10.0.0.0/8"]
            scopes = ["*"]
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();

        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        assert_eq!(config.matches_whitelist(ip).unwrap(), &["ha:read"]);

        let ip: IpAddr = "10.5.3.1".parse().unwrap();
        assert_eq!(config.matches_whitelist(ip).unwrap(), &["*"]);

        let ip: IpAddr = "172.16.0.1".parse().unwrap();
        assert!(config.matches_whitelist(ip).is_none());
    }

    #[test]
    fn z2m_default_base_topic() {
        let toml_str = r#"
            [z2m]
            mqtt_host = "mqtt://localhost:1883"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.z2m.unwrap().base_topic, "zigbee2mqtt");
    }
}
