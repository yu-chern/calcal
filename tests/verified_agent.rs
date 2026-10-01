//! Integration checks for the production protocol. No live model and no production DB.
use async_trait::async_trait;
use calcal::{
    agent::{AgentService, config::SystemConfig},
    models::{ModelAdapter, ModelRequest, ModelTurn, ToolCall},
    storage::Store,
    tools::ToolRegistry,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;
struct Scripted {
    turns: Vec<ModelTurn>,
    calls: AtomicUsize,
}
#[async_trait]
impl ModelAdapter for Scripted {
    async fn generate(&self, request: &ModelRequest) -> Result<ModelTurn, String> {
        assert!(request.instructions.contains("服务端提供的输入来源列表"));
        let i = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.turns[i.min(self.turns.len() - 1)].clone())
    }
}
fn turn(calls: Vec<ToolCall>, text: &str) -> ModelTurn {
    ModelTurn {
        text: text.into(),
        tool_calls: calls,
        refused: false,
        usage: json!({}),
        continuation: json!([]),
    }
}
fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: Uuid::new_v4().to_string(),
        name: name.into(),
        arguments: args,
    }
}
fn program() -> Value {
    json!({"inputs":[{"id":"x","source":0,"quote":"0.1+0.2","kind":"expression"}],"steps":[{"id":"answer","expression":"x"}],"outputs":[{"ref":"answer","label":"result","unit":"none"}]})
}
async fn terminal(store: &Store, owner: &str, id: Uuid) -> calcal::storage::RunView {
    for _ in 0..200 {
        let r = store.view(owner, id).await.unwrap();
        if r.status != "running" {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timeout")
}
async fn service(
    store: &Store,
    turns: Vec<ModelTurn>,
    max_steps: usize,
) -> (Arc<AgentService>, Arc<Scripted>) {
    let mut cfg = SystemConfig::load(Path::new("system_config/config.toml")).unwrap();
    cfg.agent.max_steps = max_steps;
    cfg.agent.history_turns = 1;
    let tools = ToolRegistry::load(Path::new("system_config"), &cfg.tools.definitions).unwrap();
    let model = Arc::new(Scripted {
        turns,
        calls: AtomicUsize::new(0),
    });
    (
        AgentService::new(store.clone(), cfg, model.clone(), tools),
        model,
    )
}
#[tokio::test]
#[ignore = "isolated TEST_DATABASE_URL required; run scripts/test-db.sh"]
async fn verified_answers_fail_closed_repair_and_resume() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    assert!(reqwest::Url::parse(&url).unwrap().path().ends_with("_test"));
    assert!(!std::env::var("DATABASE_URL").is_ok_and(|s| s == url));
    let store = Store::connect(&url).await.unwrap();
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let owner = format!("verified-test-{}", Uuid::new_v4());
    // A successful program is terminal in the last allowed step. Model prose is ignored.
    let (agent, model) = service(
        &store,
        vec![turn(vec![call("compute", program())], "答案是999999")],
        1,
    )
    .await;
    let id = Uuid::new_v4();
    agent
        .submit(&owner, Uuid::new_v4(), id, "计算0.1+0.2")
        .await
        .unwrap();
    let v = terminal(&store, &owner, id).await;
    assert_eq!(v.reason.as_deref(), Some("completed"));
    assert_eq!(v.response.as_deref(), Some("结果：0.3"));
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let evidence: Value = sqlx::query_scalar(
        "SELECT payload FROM conversation_entries WHERE run_id=$1 AND kind='answer_verified'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(evidence["claims"][0]["data"]["value"], "0.3");
    agent.shutdown().await;
    // Plain model answers never reach the user, including after a failed tool.
    for turns in [
        vec![turn(vec![], "结果：1680")],
        vec![
            turn(
                vec![call(
                    "compute",
                    json!({"inputs":[],"steps":[{"id":"bad","expression":"1680"}],"outputs":[{"ref":"bad","label":"result","unit":"none"}]}),
                )],
                "",
            ),
            turn(vec![], "结果：1680"),
        ],
    ] {
        let (agent, _) = service(&store, turns, 2).await;
        let id = Uuid::new_v4();
        agent
            .submit(&owner, Uuid::new_v4(), id, "计算0.1+0.2")
            .await
            .unwrap();
        let v = terminal(&store, &owner, id).await;
        assert_eq!(v.reason.as_deref(), Some("budget_exhausted"));
        assert!(v.response.is_none());
        agent.shutdown().await;
    }
    // A rejected response may be repaired within the bounded loop.
    let (agent, model) = service(
        &store,
        vec![
            turn(vec![], "结果是14"),
            turn(vec![call("compute", program())], ""),
        ],
        2,
    )
    .await;
    let id = Uuid::new_v4();
    agent
        .submit(&owner, Uuid::new_v4(), id, "计算0.1+0.2")
        .await
        .unwrap();
    assert_eq!(
        terminal(&store, &owner, id).await.response.as_deref(),
        Some("结果：0.3")
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    agent.shutdown().await;
    // Invalid outputs cannot produce answer_verified even if all arithmetic succeeded.
    let mut bad = program();
    bad["outputs"][0]["ref"] = json!("x");
    let (agent, _) = service(&store, vec![turn(vec![call("compute", bad)], "")], 1).await;
    let id = Uuid::new_v4();
    agent
        .submit(&owner, Uuid::new_v4(), id, "计算0.1+0.2")
        .await
        .unwrap();
    assert!(terminal(&store, &owner, id).await.response.is_none());
    agent.shutdown().await;
    // Clarification wins over a requested calculation; the original source survives history_turns=1.
    let conversation = Uuid::new_v4();
    let id = Uuid::new_v4();
    let (agent, _) = service(
        &store,
        vec![turn(
            vec![
                call("compute", program()),
                call(
                    "clarify",
                    json!({"topic":"conditions","source":0,"quote":"0.1+0.2"}),
                ),
            ],
            "未验证结论",
        )],
        1,
    )
    .await;
    agent
        .submit(&owner, conversation, id, "计算0.1+0.2")
        .await
        .unwrap();
    let v = terminal(&store, &owner, id).await;
    assert_eq!(v.reason.as_deref(), Some("clarification"));
    assert!(!v.response.unwrap().contains("未验证"));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM conversation_entries WHERE run_id=$1 AND kind='tool_call'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    agent.shutdown().await;
    let (agent, _) = service(&store, vec![turn(vec![call("compute", program())], "")], 1).await;
    let id = Uuid::new_v4();
    agent
        .submit(&owner, conversation, id, "请使用精确小数")
        .await
        .unwrap();
    assert_eq!(
        terminal(&store, &owner, id).await.response.as_deref(),
        Some("结果：0.3")
    );
    agent.shutdown().await;
    // No source can be injected from an assistant message or a nonexistent source ID.
    let mut bad = program();
    bad["inputs"][0]["source"] = json!(2);
    let (agent, _) = service(&store, vec![turn(vec![call("compute", bad)], "")], 1).await;
    let id = Uuid::new_v4();
    agent
        .submit(&owner, Uuid::new_v4(), id, "计算0.1+0.2")
        .await
        .unwrap();
    assert!(terminal(&store, &owner, id).await.response.is_none());
    agent.shutdown().await;
    // Disabling a tool cannot disable the independent answer-validation policy.
    let (agent, _) = service(&store, vec![turn(vec![], "计算结果：999")], 1).await;
    agent.tools.write().await.disable("compute");
    let id = Uuid::new_v4();
    agent
        .submit(&owner, Uuid::new_v4(), id, "计算0.1+0.2")
        .await
        .unwrap();
    assert!(terminal(&store, &owner, id).await.response.is_none());
    agent.shutdown().await;
}
