use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ha::HaClient;
use crate::z2m::Z2mClient;

#[derive(Clone)]
pub struct BackendStatus {
    ha_available: Option<Arc<AtomicBool>>,
    z2m_client: Option<Z2mClient>,
    ha_client: Option<HaClient>,
}

impl BackendStatus {
    pub fn new(ha: Option<HaClient>, z2m: Option<Z2mClient>) -> Self {
        let ha_available = ha.as_ref().map(|_| Arc::new(AtomicBool::new(false)));

        let status = Self {
            ha_available,
            z2m_client: z2m,
            ha_client: ha,
        };

        if let (Some(client), Some(flag)) = (&status.ha_client, &status.ha_available) {
            let client = client.clone();
            let flag = flag.clone();
            tokio::spawn(async move {
                ha_health_loop(client, flag).await;
            });
        }

        status
    }

    pub fn ha_available(&self) -> Option<bool> {
        self.ha_available
            .as_ref()
            .map(|f| f.load(Ordering::Relaxed))
    }

    pub async fn z2m_available(&self) -> Option<bool> {
        match &self.z2m_client {
            Some(client) => Some(client.is_connected().await),
            None => None,
        }
    }

    pub fn ha_configured(&self) -> bool {
        self.ha_client.is_some()
    }

    pub fn z2m_configured(&self) -> bool {
        self.z2m_client.is_some()
    }

    pub fn tool_backend_available(&self, tool_name: &str) -> Option<bool> {
        if tool_name.starts_with("ha_") {
            self.ha_available()
        } else if tool_name.starts_with("z2m_") {
            // Z2M availability check is async; for sync context, return None
            // The async check happens in the middleware
            None
        } else {
            Some(true)
        }
    }
}

async fn ha_health_loop(client: HaClient, flag: Arc<AtomicBool>) {
    loop {
        let ok = client.check_connection().await.is_ok();
        let was = flag.swap(ok, Ordering::Relaxed);
        if ok != was {
            if ok {
                tracing::info!("HA backend recovered");
            } else {
                tracing::warn!("HA backend unreachable");
            }
        }
        let interval = if ok { 30 } else { 5 };
        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}
