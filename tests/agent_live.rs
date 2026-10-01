//! Explicit opt-in evaluation; never runs in the offline or standard DB suites.
use calcal::{
    agent::{AgentService, config::SystemConfig},
    models::openai::OpenAi,
    storage::Store,
    tools::ToolRegistry,
};
use serde_json::Value;
use std::{path::Path, sync::Arc, time::Duration};
use uuid::Uuid;

#[tokio::test]
#[ignore = "uses live model credits and an isolated TEST_DATABASE_URL; CALCAL_LIVE_EVAL=1 cargo test --test agent_live -- --ignored --nocapture"]
async fn date_math_and_clarification_live_evaluation() {
    assert_eq!(std::env::var("CALCAL_LIVE_EVAL").as_deref(), Ok("1"));
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path("deploy/runtime/postgres.env");
    let url = std::env::var("TEST_DATABASE_URL").expect("isolated TEST_DATABASE_URL required");
    let parsed = reqwest::Url::parse(&url).expect("invalid test database URL");
    assert!(
        parsed.path().ends_with("_test"),
        "live evaluation requires a database named *_test"
    );
    assert!(
        !std::env::var("DATABASE_URL").is_ok_and(|production| production == url),
        "cannot use production database"
    );
    let config = SystemConfig::load(Path::new("system_config/config.toml")).unwrap();
    let key = std::env::var(&config.model.api_key_env).expect("server-side model key required");
    let model = Arc::new(OpenAi::new(config.model.clone(), key).unwrap());
    let tools = ToolRegistry::load(Path::new("system_config"), &config.tools.definitions).unwrap();
    let store = Store::connect(&url).await.unwrap();
    let pool = sqlx::PgPool::connect(&url)
        .await
        .expect("test DB connection failed");
    let agent = AgentService::new(store.clone(), config, model, tools);
    let owner = format!("live-eval-{}", Uuid::new_v4());
    let cases = [
        (
            "today",
            "今天的实际日期是哪一天？只说日期。",
            "completed",
            vec![],
            None,
        ),
        (
            "arithmetic",
            "计算 (18.5 + 7.25) * 12。",
            "completed",
            vec!["309"],
            Some("calculator"),
        ),
        (
            "batch_math",
            "分别计算 sqrt(144)、(25+15)*3、2^10。",
            "completed",
            vec!["12", "120", "1024"],
            Some("compute"),
        ),
        (
            "date_math",
            "2026-10-01到2026-10-15按end-start的天数计费，每天120元，共多少元？",
            "completed",
            vec!["1680"],
            Some("compute"),
        ),
        (
            "date_search",
            "以2026-10-01为参考，只向过去找，最近的闰年二月最后一天为星期五是哪一天？",
            "completed",
            vec!["2008"],
            Some("compute"),
        ),
        (
            "ambiguous",
            "最近一个2月最后一天为星期五的闰年是哪年？",
            "clarification",
            vec![],
            Some("clarify"),
        ),
        (
            "conflicting",
            "日期必须既在2026年10月1日之前，又在2026年10月15日之后，帮我挑最近一天。",
            "clarification",
            vec![],
            Some("clarify"),
        ),
        (
            "month_end",
            "2026年1月31日加一个月是哪一天？",
            "clarification",
            vec![],
            Some("clarify"),
        ),
        (
            "precision",
            "精确计算9007199254740993+1，必须输出准确整数，不能使用近似值。",
            "clarification",
            vec![],
            Some("clarify"),
        ),
    ];
    let mut failures = vec![];
    let clarification_conversation = Uuid::new_v4();
    for (name, prompt, reason, expected, tool) in cases.into_iter().chain(std::iter::once((
        "clarification_resume",
        "只向过去找，以2026年10月1日为参考。",
        "completed",
        vec!["2008"],
        Some("compute"),
    ))) {
        if let Ok(selected) = std::env::var("CALCAL_LIVE_CASES")
            && !selected.split(',').any(|s| s == name)
        {
            continue;
        }
        let conversation = if matches!(name, "ambiguous" | "clarification_resume") {
            clarification_conversation
        } else {
            Uuid::new_v4()
        };
        let id = Uuid::new_v4();
        agent
            .submit(&owner, conversation, id, prompt)
            .await
            .unwrap();
        let start = std::time::Instant::now();
        let view = loop {
            let v = store.view(&owner, id).await.unwrap();
            if v.status != "running" {
                break v;
            }
            assert!(
                start.elapsed() < Duration::from_secs(110),
                "run exceeded evaluation deadline"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM conversation_entries WHERE run_id=$1 AND kind='model_turn'",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let names:Vec<String>=sqlx::query_scalar("SELECT payload->'call'->>'name' FROM conversation_entries WHERE run_id=$1 AND kind='tool_call' ORDER BY sequence").bind(id).fetch_all(&pool).await.unwrap();
        let results:Vec<Value>=sqlx::query_scalar("SELECT payload->'result' FROM conversation_entries WHERE run_id=$1 AND kind='tool_result' ORDER BY sequence").bind(id).fetch_all(&pool).await.unwrap();
        let text = view
            .response
            .as_deref()
            .unwrap_or("")
            .replace([',', '，'], "");
        let minimal_search = results.iter().all(|result| {
            result["data"]["steps"].as_array().is_none_or(|steps| {
                steps.iter().all(|s| {
                    s["action"]["kind"] != "find_dates" || s["action"]["query"]["limit"] == 1
                })
            })
        });
        let snapshot: Value = sqlx::query_scalar("SELECT snapshot FROM runs WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let today_correct = name != "today"
            || view.response.as_deref() == snapshot["runtime_context"]["today"].as_str();
        let no_premature_result = name != "month_end"
            || !["2月28", "02-28", "3月3", "03-03"]
                .iter()
                .any(|s| text.contains(s));
        let pass = view.reason.as_deref() == Some(reason)
            && expected.iter().all(|s| text.contains(s))
            && count <= if reason == "clarification" { 1 } else { 2 }
            && tool.is_none_or(|t| names == vec![t])
            && results.iter().all(|r| r["ok"] == true)
            && minimal_search
            && today_correct
            && no_premature_result
            && (name != "today" || (count == 1 && names.is_empty()));
        println!(
            "EVAL {}",
            serde_json::json!({"name":name,"pass":pass,"run_id":id,"reason":view.reason,"error":view.error,"llm_calls":count,"tools":names,"elapsed_ms":start.elapsed().as_millis(),"response":view.response,"tool_results":results})
        );
        if !pass {
            failures.push(name);
        }
    }
    agent.shutdown().await;
    assert!(
        failures.is_empty(),
        "live evaluation failures: {failures:?}"
    );
}
