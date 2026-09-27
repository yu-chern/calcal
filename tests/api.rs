use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use calcal::{
    access::Access,
    config::{AuthConfig, Config},
    http::router,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app() -> axum::Router {
    router(
        Config {
            port: 3000,
            origin: "http://127.0.0.1:5173".into(),
            web_dir: "web/dist".into(),
            auth: AuthConfig::Local,
        },
        Access::Local,
    )
}

async fn post(body: String, origin: Option<&str>) -> axum::response::Response {
    let mut request = Request::post("/api/messages")
        .header("host", "127.0.0.1:3000")
        .header("content-type", "application/json");
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    app()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn accepts_unicode_and_multiline_prompts() {
    let response = post(
        serde_json::json!({"prompt":"  你好 🦀\nRust!  "}).to_string(),
        Some("http://127.0.0.1:5173"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["status"],
        "printed"
    );
}

#[tokio::test]
async fn rejects_empty_oversized_and_malformed_prompts() {
    for prompt in [" \n\t".into(), "你".repeat(4001)] {
        assert_eq!(
            post(
                serde_json::json!({"prompt":prompt}).to_string(),
                Some("http://127.0.0.1:5173")
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        post("not json".into(), Some("http://127.0.0.1:5173"))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(
            serde_json::json!({"prompt":"a".repeat(66000)}).to_string(),
            Some("http://127.0.0.1:5173")
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn rejects_cross_origin_and_missing_origin() {
    for origin in [None, Some("https://evil.example")] {
        assert_eq!(
            post("{\"prompt\":\"hello\"}".into(), origin).await.status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn local_mode_rejects_public_hosts_and_cloudflare_traffic() {
    for (host, proxy) in [("finanio.app", false), ("127.0.0.1:3000", true)] {
        let mut request = Request::get("/api/session").header("host", host);
        if proxy {
            request = request.header("cf-ray", "test");
        }
        assert_eq!(
            app()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn session_exposes_only_demo_capabilities() {
    let response = app()
        .oneshot(
            Request::get("/api/session")
                .header("host", "localhost:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["mode"],
        "print"
    );
}
