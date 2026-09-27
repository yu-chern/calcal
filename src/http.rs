use crate::{access::Access, config::Config, prompt::print_prompt};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

#[derive(Clone)]
struct AppState {
    origin: String,
    access: Access,
}

pub fn router(config: Config, access: Access) -> Router {
    let state = AppState {
        origin: config.origin,
        access,
    };
    Router::new()
        .route("/api/session", get(session))
        .route("/api/messages", post(message))
        .route("/api/{*path}", get(|| async { error(StatusCode::NOT_FOUND, "接口不存在") }))
        .fallback_service(ServeDir::new(config.web_dir))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .layer(ConcurrencyLimitLayer::new(16))
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store")))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, header::HeaderValue::from_static("no-referrer")))
        .layer(SetResponseHeaderLayer::overriding(header::CONTENT_SECURITY_POLICY, header::HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")))
        .with_state(state)
}

async fn guard(State(state): State<AppState>, request: Request<Body>, next: Next) -> Response {
    let headers = request.headers();
    if matches!(state.access, Access::Local) {
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        let loopback_host = host
            .split(':')
            .next()
            .is_some_and(|h| matches!(h, "localhost" | "127.0.0.1"));
        if !loopback_host
            || headers.contains_key("cf-ray")
            || headers.contains_key("cf-connecting-ip")
        {
            return error(StatusCode::FORBIDDEN, "本地开发模式不接受远程请求");
        }
    }
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(state.origin.as_str())
    {
        return error(StatusCode::FORBIDDEN, "请求来源不被允许");
    }
    let token = headers
        .get("cf-access-jwt-assertion")
        .and_then(|v| v.to_str().ok());
    if state.access.authenticate(token).await.is_none() {
        return error(StatusCode::UNAUTHORIZED, "登录已失效，请刷新页面重新登录");
    }
    match tokio::time::timeout(std::time::Duration::from_secs(10), next.run(request)).await {
        Ok(response) => response,
        Err(_) => error(StatusCode::REQUEST_TIMEOUT, "请求超时"),
    }
}

async fn session(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"mode": "print", "auth": state.access.mode(), "max_prompt_chars": 4000}),
    )
}

#[derive(Deserialize)]
struct Message {
    prompt: String,
}

#[derive(Serialize)]
struct Receipt {
    status: &'static str,
    message: &'static str,
}

async fn message(payload: Result<Json<Message>, JsonRejection>) -> Response {
    let Json(message) = match payload {
        Ok(value) => value,
        Err(rejection) => {
            return error(
                rejection.status(),
                "请发送有效 JSON，包含字符串字段 prompt（请求体不超过 64 KiB）",
            );
        }
    };
    if message.prompt.trim().is_empty() || message.prompt.chars().count() > 4000 {
        return error(
            StatusCode::BAD_REQUEST,
            "消息不能为空，且最多为 4000 个字符",
        );
    }
    print_prompt(&message.prompt);
    Json(Receipt {
        status: "printed",
        message: "消息已在 Rust 后端打印。",
    })
    .into_response()
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
