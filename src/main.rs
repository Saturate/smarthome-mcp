use tokio_util::sync::CancellationToken;

use smarthome_mcp::config::Config;
use smarthome_mcp::create_mcp_router;
use smarthome_mcp::ha::HaClient;
use smarthome_mcp::z2m::Z2mClient;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::load();

    let ha = config.ha.as_ref().map(|ha_config| {
        tracing::info!(url = %ha_config.url, "Home Assistant backend configured");
        HaClient::new(ha_config)
    });

    let z2m = if let Some(ref z2m_config) = config.z2m {
        tracing::info!(host = %z2m_config.mqtt_host, topic = %z2m_config.base_topic, "Zigbee2MQTT backend configured");
        match Z2mClient::new(z2m_config).await {
            Ok(client) => Some(client),
            Err(e) => {
                tracing::warn!(error = %e, "failed to connect to Zigbee2MQTT, starting without Z2M");
                None
            }
        }
    } else {
        None
    };

    if config.is_open_auth() {
        tracing::warn!("no auth configured, all requests allowed");
    }

    let port = config.server.port;
    let ct = CancellationToken::new();
    let router = create_mcp_router(ha, z2m);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind TCP listener");

    tracing::info!("smarthome-mcp listening on http://0.0.0.0:{port}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await
        .expect("server error");
}
