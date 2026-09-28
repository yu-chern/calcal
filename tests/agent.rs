use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use calcal::{
    access::Access,
    agent::{AgentService, config::SystemConfig},
    config::{AuthConfig, Config},
    models::{ModelAdapter, ModelRequest, ModelTurn, ToolCall},
    storage::{Store, StoreError},
    tools::ToolRegistry,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;
use uuid::Uuid;

struct Scripted {
    mode: &'static str,
    calls: AtomicUsize,
}
#[async_trait]
impl ModelAdapter for Scripted {
    async fn generate(&self, request: &ModelRequest) -> Result<ModelTurn, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(if self.mode == "slow" {
            2000
        } else {
            30
        }))
        .await;
        if self.mode == "error" {
            return Err("测试模型不可用".into());
        }
        let n = request.turns.len();
        let (text, tool_calls) = if self.mode == "loop" {
            (
                "",
                vec![ToolCall {
                    id: format!("c{n}"),
                    name: "missing".into(),
                    arguments: json!({}),
                }],
            )
        } else if n == 0 {
            (
                "",
                vec![ToolCall {
                    id: "date".into(),
                    name: "calendar".into(),
                    arguments: json!({"operation":"diff","date":null,"start":"2026-09-28","end":"2026-10-12","amount":null,"unit":null}),
                }],
            )
        } else if n == 1 {
            assert_eq!(request.outputs[0][0].result["data"]["days"], 14);
            (
                "",
                vec![ToolCall {
                    id: "math".into(),
                    name: "calculator".into(),
                    arguments: json!({"expression":"14 * 120"}),
                }],
            )
        } else {
            assert_eq!(request.outputs[1][0].result["data"]["value"], 1680.0);
            ("相差14天，共1680元。", vec![])
        };
        Ok(ModelTurn {
            text: text.into(),
            tool_calls,
            refused: false,
            usage: json!({}),
            continuation: json!([]),
        })
    }
}
fn config() -> SystemConfig {
    SystemConfig::load(Path::new("system_config/config.toml")).unwrap()
}
fn service(
    store: Store,
    mode: &'static str,
    mut config: SystemConfig,
) -> (Arc<AgentService>, Arc<Scripted>) {
    if mode == "slow" {
        config.agent.run_timeout_seconds = 1;
    }
    let tools = ToolRegistry::load(Path::new("system_config"), &config.tools.definitions).unwrap();
    let model = Arc::new(Scripted {
        mode,
        calls: AtomicUsize::new(0),
    });
    (
        AgentService::new(store, config, model.clone(), tools),
        model,
    )
}
async fn terminal(store: &Store, owner: &str, id: Uuid) -> calcal::storage::RunView {
    for _ in 0..150 {
        let run = store.view(owner, id).await.unwrap();
        if run.status != "running" {
            return run;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("run did not finish")
}
#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL; run scripts/test-db.sh"]
async fn persistent_agent_http_and_failure_boundaries() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    // The suite creates only uniquely identified test conversations, never deletes data.
    let store = Store::connect(&url).await.unwrap();
    assert!(
        Store::connect(&url).await.is_err(),
        "second runtime must not recover live tasks"
    );
    let (agent, model) = service(store.clone(), "success", config());
    let app = calcal::http::router(
        Config {
            port: 3000,
            origin: "http://127.0.0.1:5180".into(),
            web_dir: "web/dist".into(),
            auth: AuthConfig::Local,
        },
        Access::Local,
        Some(agent.clone()),
    );
    let conversation = Uuid::new_v4();
    let id = Uuid::new_v4();
    let body =
        json!({"conversation_id":conversation,"request_id":id,"prompt":"  你好\n计算日期与费用  "})
            .to_string();
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/messages")
                    .header("host", "127.0.0.1:3000")
                    .header("origin", "http://127.0.0.1:5180")
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }
    assert!(
        agent
            .submit("本地开发", conversation, Uuid::new_v4(), "overlap")
            .await
            .is_err()
    );
    let view = terminal(&store, "本地开发", id).await;
    assert_eq!(view.response.as_deref(), Some("相差14天，共1680元。"));
    assert_eq!(view.prompt, "  你好\n计算日期与费用  ");
    assert!(view.activities.contains(&"正在查询日期".into()));
    assert!(view.activities.contains(&"正在计算".into()));
    assert_eq!(model.calls.load(Ordering::SeqCst), 3);
    assert!(matches!(
        store.view("another-owner", id).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.conversation("another-owner", conversation, 0).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        agent
            .submit("another-owner", conversation, Uuid::new_v4(), "bad")
            .await
            .is_err()
    );
    assert!(
        agent
            .submit("本地开发", conversation, id, "different prompt")
            .await
            .is_err()
    );
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/runs/{id}"))
                .header("host", "localhost:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let public: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(public.get("continuation").is_none());
    assert!(public.get("snapshot").is_none());
    assert_eq!(
        store
            .history(conversation, id, 12, 48000)
            .await
            .unwrap()
            .len(),
        2
    );
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM conversation_entries WHERE run_id=$1 AND kind='user_message'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "idempotent submission must not duplicate messages"
    );
    let results: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM conversation_entries WHERE run_id=$1 AND kind='tool_result'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(results, 2);
    agent.shutdown().await;
    for (mode, reason) in [
        ("error", "model_error"),
        ("slow", "timeout"),
        ("loop", "budget_exhausted"),
    ] {
        let mut cfg = config();
        cfg.agent.max_steps = 2;
        let (service, _) = service(store.clone(), mode, cfg);
        let run = Uuid::new_v4();
        service
            .submit("tester", Uuid::new_v4(), run, "failure test")
            .await
            .unwrap();
        let view = terminal(&store, "tester", run).await;
        assert_eq!(view.status, "failed");
        assert_eq!(view.reason.as_deref(), Some(reason));
        assert!(view.response.is_none());
        service.shutdown().await;
    }
    let (service, _) = service(store.clone(), "slow", config());
    let run = Uuid::new_v4();
    service
        .submit("tester", Uuid::new_v4(), run, "cancel")
        .await
        .unwrap();
    service.shutdown().await;
    assert_eq!(
        terminal(&store, "tester", run).await.reason.as_deref(),
        Some("cancelled")
    );
    let orphan = Uuid::new_v4();
    store
        .begin(
            "tester",
            Uuid::new_v4(),
            orphan,
            "orphan",
            json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    sqlx::query("UPDATE runs SET deadline_at=now()-interval '1 second' WHERE id=$1")
        .bind(orphan)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        store
            .view("tester", orphan)
            .await
            .unwrap()
            .reason
            .as_deref(),
        Some("interrupted")
    );
}
