use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn mcp_endpoint_rejects_non_json_rpc() {
    let app = smarthome_mcp::create_router();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"not": "jsonrpc"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_returns_ok() {
    let app = smarthome_mcp::create_router();
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // GET without session should return 400
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
