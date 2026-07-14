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

#[derive(Clone)]
struct SmartHomeMcp;

#[derive(Debug, Deserialize, JsonSchema)]
struct PingParams {
    #[schemars(description = "Optional message to echo back")]
    message: Option<String>,
}

#[tool_router(server_handler)]
impl SmartHomeMcp {
    #[tool(name = "ping", description = "Health check ping, returns pong")]
    async fn ping(&self, Parameters(params): Parameters<PingParams>) -> String {
        match params.message {
            Some(msg) => format!("pong: {msg}"),
            None => "pong".to_string(),
        }
    }
}

pub fn create_router() -> axum::Router {
    let ct = CancellationToken::new();
    let config = StreamableHttpServerConfig::default().with_cancellation_token(ct);
    let session_manager = Arc::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(|| Ok(SmartHomeMcp), session_manager, config);
    axum::Router::new().nest_service("/mcp", service)
}
