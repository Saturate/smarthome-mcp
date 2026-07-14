use tokio_util::sync::CancellationToken;

use smarthome_mcp::config::Config;
use smarthome_mcp::create_router_with_ha;
use smarthome_mcp::ha::HaClient;

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

    if let Some(ref z2m) = config.z2m {
        tracing::info!(host = %z2m.mqtt_host, topic = %z2m.base_topic, "Zigbee2MQTT backend configured");
    }

    if config.is_open_auth() {
        tracing::warn!("no auth configured, all requests allowed");
    }

    let port = config.server.port;
    let ct = CancellationToken::new();
    let router = create_router_with_ha(ha);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind TCP listener");

    tracing::info!("smarthome-mcp listening on http://0.0.0.0:{port}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await
        .expect("server error");
}
