use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::auth::scopes::{Scope, parse_scopes};
use crate::config::ProxyAuthConfig;

#[derive(Clone)]
pub struct ProxyValidator {
    config: ProxyAuthConfig,
    cache: Arc<RwLock<Vec<CachedToken>>>,
    ha_url: Option<String>,
}

struct CachedToken {
    token: String,
    scopes: HashSet<Scope>,
    expires_at: Instant,
}

impl ProxyValidator {
    pub fn new(config: ProxyAuthConfig, ha_url: Option<String>) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(Vec::new())),
            ha_url,
        }
    }

    pub async fn validate(&self, bearer: &str) -> Option<HashSet<Scope>> {
        if self.config.cache_ttl > 0
            && let Some(scopes) = self.check_cache(bearer).await
        {
            return Some(scopes);
        }

        let valid = match self.config.backend {
            crate::config::ProxyBackend::Ha => self.validate_ha(bearer).await,
            crate::config::ProxyBackend::Mqtt => self.validate_mqtt(bearer).await,
        };

        if valid {
            let scopes = parse_scopes(&self.config.scopes);
            if self.config.cache_ttl > 0 {
                self.insert_cache(bearer, &scopes).await;
            }
            Some(scopes)
        } else {
            None
        }
    }

    async fn check_cache(&self, bearer: &str) -> Option<HashSet<Scope>> {
        let cache = self.cache.read().await;
        let now = Instant::now();
        cache
            .iter()
            .find(|entry| entry.token == bearer && entry.expires_at > now)
            .map(|entry| entry.scopes.clone())
    }

    async fn insert_cache(&self, bearer: &str, scopes: &HashSet<Scope>) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();
        cache.retain(|entry| entry.expires_at > now);
        cache.push(CachedToken {
            token: bearer.to_string(),
            scopes: scopes.clone(),
            expires_at: now + Duration::from_secs(self.config.cache_ttl),
        });
    }

    async fn validate_ha(&self, bearer: &str) -> bool {
        let Some(ref ha_url) = self.ha_url else {
            tracing::warn!("proxy auth configured for HA but no HA_URL set");
            return false;
        };
        let url = format!("{}/api/", ha_url.trim_end_matches('/'));
        let client = reqwest::Client::new();
        match client
            .get(&url)
            .header("Authorization", format!("Bearer {bearer}"))
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                tracing::warn!(error = %e, "proxy auth HA validation failed");
                false
            }
        }
    }

    async fn validate_mqtt(&self, _bearer: &str) -> bool {
        // MQTT proxy validation will be implemented with the Z2M client
        tracing::warn!("MQTT proxy auth not yet implemented");
        false
    }
}
