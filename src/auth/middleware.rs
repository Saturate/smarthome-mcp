use std::convert::Infallible;
use std::net::SocketAddr;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;

use super::scopes::tool_required_scope;
use super::AuthResolver;

#[derive(Clone)]
pub struct AuthLayer {
    resolver: AuthResolver,
}

impl AuthLayer {
    pub fn new(resolver: AuthResolver) -> Self {
        Self { resolver }
    }
}

impl<S> tower::Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            resolver: self.resolver.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    resolver: AuthResolver,
}

impl<S, ResBody> tower::Service<Request<Body>> for AuthService<S>
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
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let resolver = self.resolver.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
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

            if request.method() != axum::http::Method::POST {
                let resp = inner.call(request).await?;
                return Ok(resp.map(|b| Body::new(b)));
            }

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
                && let Some(denied) = check_tool_call_auth(&json, &scopes)
            {
                return Ok(denied);
            }

            let request = Request::from_parts(parts, Body::from(bytes));
            let resp = inner.call(request).await?;
            Ok(resp.map(|b| Body::new(b)))
        })
    }
}

fn check_tool_call_auth(
    json: &serde_json::Value,
    scopes: &super::GrantedScopes,
) -> Option<Response<Body>> {
    if json.get("method").and_then(|m| m.as_str()) != Some("tools/call") {
        return None;
    }

    let tool_name = json
        .get("params")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())?;

    let required = tool_required_scope(tool_name)?;

    if scopes.has(&required) {
        return None;
    }

    let request_id = json.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let error_body = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": -32001,
            "message": format!(
                "missing scope '{}' required for tool '{}'",
                required.as_str(),
                tool_name
            ),
        },
        "id": request_id,
    });

    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&error_body).unwrap()))
            .unwrap(),
    )
}
