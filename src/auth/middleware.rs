use std::convert::Infallible;
use std::net::SocketAddr;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;

use super::AuthResolver;
use super::scopes::tool_required_scope;
use crate::backend_status::BackendStatus;

fn jsonrpc_error(id: serde_json::Value, code: i32, message: String) -> Response<Body> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "error": { "code": code, "message": message },
        "id": id,
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[derive(Clone)]
pub struct AuthLayer {
    resolver: AuthResolver,
    backend_status: Option<BackendStatus>,
}

impl AuthLayer {
    pub fn new(resolver: AuthResolver) -> Self {
        Self {
            resolver,
            backend_status: None,
        }
    }

    pub fn with_backend_status(mut self, status: BackendStatus) -> Self {
        self.backend_status = Some(status);
        self
    }
}

impl<S> tower::Layer<S> for AuthLayer {
    type Service = McpGuardService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        McpGuardService {
            inner,
            resolver: self.resolver.clone(),
            backend_status: self.backend_status.clone(),
        }
    }
}

#[derive(Clone)]
pub struct BackendCheckLayer {
    status: BackendStatus,
}

impl BackendCheckLayer {
    pub fn new(status: BackendStatus) -> Self {
        Self { status }
    }
}

impl<S> tower::Layer<S> for BackendCheckLayer {
    type Service = McpGuardService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        McpGuardService {
            inner,
            resolver: AuthResolver::open(),
            backend_status: Some(self.status.clone()),
        }
    }
}

#[derive(Clone)]
pub struct McpGuardService<S> {
    inner: S,
    resolver: AuthResolver,
    backend_status: Option<BackendStatus>,
}

impl<S, ResBody> tower::Service<Request<Body>> for McpGuardService<S>
where
    S: tower::Service<Request<Body>, Response = Response<ResBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    ResBody: http_body::Body<Data = bytes::Bytes, Error = Infallible> + Send + 'static,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let resolver = self.resolver.clone();
        let backend_status = self.backend_status.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if request.method() != axum::http::Method::POST {
                let resp = inner.call(request).await?;
                return Ok(resp.map(|b| Body::new(b)));
            }

            let addr = request
                .extensions()
                .get::<axum::extract::ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0)
                .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0)));

            let bearer = request
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(|s| s.to_string());

            let scopes = resolver.resolve(addr, bearer.as_deref()).await;

            let (parts, body) = request.into_parts();
            let bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    let resp = inner
                        .call(Request::from_parts(parts, Body::empty()))
                        .await?;
                    return Ok(resp.map(|b| Body::new(b)));
                }
            };

            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(tool_name) = extract_tool_name(&json)
            {
                let request_id = json.get("id").cloned().unwrap_or(serde_json::Value::Null);

                if let Some(required) = tool_required_scope(tool_name)
                    && !scopes.has(&required)
                {
                    return Ok(jsonrpc_error(
                        request_id,
                        -32001,
                        format!(
                            "missing scope '{}' required for tool '{tool_name}'",
                            required.as_str()
                        ),
                    ));
                }

                if let Some(ref status) = backend_status
                    && let Some(denied) =
                        check_backend_availability(status, tool_name, request_id).await
                {
                    return Ok(denied);
                }
            }

            let request = Request::from_parts(parts, Body::from(bytes));
            let resp = inner.call(request).await?;
            Ok(resp.map(|b| Body::new(b)))
        })
    }
}

fn extract_tool_name(json: &serde_json::Value) -> Option<&str> {
    if json.get("method").and_then(|m| m.as_str()) != Some("tools/call") {
        return None;
    }
    json.get("params")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
}

async fn check_backend_availability(
    status: &BackendStatus,
    tool_name: &str,
    request_id: serde_json::Value,
) -> Option<Response<Body>> {
    if tool_name.starts_with("ha_") && status.ha_available() == Some(false) {
        return Some(jsonrpc_error(
            request_id,
            -32002,
            "Home Assistant backend is currently unavailable".to_string(),
        ));
    }

    if tool_name.starts_with("z2m_") && status.z2m_available().await == Some(false) {
        return Some(jsonrpc_error(
            request_id,
            -32002,
            "Zigbee2MQTT backend is currently unavailable".to_string(),
        ));
    }

    None
}
