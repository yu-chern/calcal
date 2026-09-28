use crate::{
    access::{Access, Identity},
    agent::{AgentService, SubmitError},
    config::Config,
    storage::StoreError,
};
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    origin: String,
    access: Access,
    agent: Option<Arc<AgentService>>,
}

pub fn router(config: Config, access: Access, agent: Option<Arc<AgentService>>) -> Router {
    let state = AppState {
        origin: config.origin,
        access,
        agent,
    };
    Router::new()
        .route("/api/session", get(session))
        .route("/api/messages", post(message))
        .route("/api/conversations", get(conversations))
        .route("/api/conversations/{id}", get(conversation))
        .route("/api/runs/{id}", get(run))
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

async fn guard(State(state): State<AppState>, mut request: Request<Body>, next: Next) -> Response {
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
    let Some(identity) = state.access.authenticate(token).await else {
        return error(StatusCode::UNAUTHORIZED, "登录已失效，请刷新页面重新登录");
    };
    request.extensions_mut().insert(identity);
    match tokio::time::timeout(std::time::Duration::from_secs(10), next.run(request)).await {
        Ok(response) => response,
        Err(_) => error(StatusCode::REQUEST_TIMEOUT, "请求超时"),
    }
}

async fn session(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"mode":"agent","auth":state.access.mode(),"max_prompt_chars":4000,"ready":state.agent.is_some()}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    prompt: String,
    conversation_id: Uuid,
    request_id: Uuid,
}
async fn message(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    payload: Result<Json<Message>, JsonRejection>,
) -> Response {
    let Json(message) = match payload {
        Ok(value) => value,
        Err(rejection) => {
            return error(
                rejection.status(),
                "请发送包含 prompt、conversation_id 和 request_id 的有效 JSON（不超过64 KiB）",
            );
        }
    };
    if message.prompt.trim().is_empty() || message.prompt.chars().count() > 4000 {
        return error(StatusCode::BAD_REQUEST, "消息不能为空，且最多为4000个字符");
    }
    let Some(agent) = state.agent else {
        return unavailable();
    };
    match agent
        .submit(
            &identity.email.to_lowercase(),
            message.conversation_id,
            message.request_id,
            &message.prompt,
        )
        .await
    {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"run_id":message.request_id})),
        )
            .into_response(),
        Err(SubmitError::Busy) => error(
            StatusCode::TOO_MANY_REQUESTS,
            "Agent 正在处理其他请求，请稍后再试。",
        ),
        Err(SubmitError::Store(e)) => storage_error(e),
    }
}
#[derive(Deserialize, Default)]
struct Page {
    #[serde(default)]
    offset: i64,
}
async fn conversations(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(page): Query<Page>,
) -> Response {
    if !(0..=1000000).contains(&page.offset) {
        return error(StatusCode::BAD_REQUEST, "无效分页参数");
    }
    let Some(agent) = state.agent else {
        return unavailable();
    };
    match agent
        .store
        .list(&identity.email.to_lowercase(), page.offset)
        .await
    {
        Ok(items) => Json(items).into_response(),
        Err(e) => storage_error(e),
    }
}
async fn conversation(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
) -> Response {
    if !(0..=1000000).contains(&page.offset) {
        return error(StatusCode::BAD_REQUEST, "无效分页参数");
    }
    let Some(agent) = state.agent else {
        return unavailable();
    };
    match agent
        .store
        .conversation(&identity.email.to_lowercase(), id, page.offset)
        .await
    {
        Ok(items) => Json(items).into_response(),
        Err(e) => storage_error(e),
    }
}
async fn run(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<Uuid>,
) -> Response {
    let Some(agent) = state.agent else {
        return unavailable();
    };
    match agent.store.view(&identity.email.to_lowercase(), id).await {
        Ok(item) => Json(item).into_response(),
        Err(e) => storage_error(e),
    }
}
fn unavailable() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "Agent 或数据库尚未就绪，请检查服务配置。",
    )
}
fn storage_error(e: StoreError) -> Response {
    match e {
        StoreError::NotFound => error(StatusCode::NOT_FOUND, "对话或运行不存在"),
        StoreError::Conflict => error(
            StatusCode::CONFLICT,
            "此对话已有运行中的任务，或请求 ID 与原消息不匹配。",
        ),
        StoreError::Unavailable => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "数据库暂不可用，请稍后重新连接。",
        ),
    }
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
