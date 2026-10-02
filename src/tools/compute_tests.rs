use super::*;
fn registry() -> ToolRegistry {
    ToolRegistry::load(
        Path::new("system_config"),
        &[
            "tools/calculator.v1.json".into(),
            "tools/calendar.v1.json".into(),
            "tools/compute.v1.json".into(),
            "tools/clarification.v1.json".into(),
        ],
    )
    .unwrap()
}
fn context() -> ToolContext {
    ToolContext {
        reference_time: "2026-10-01T22:30:00Z".parse().unwrap(),
        timezone: chrono_tz::Europe::Berlin,
        sources: vec![],
        selections: std::collections::BTreeMap::new(),
        verified_answers: Default::default(),
    }
}
async fn run(args: Value) -> Value {
    registry()
        .execute("compute", args, &context(), Duration::from_secs(2))
        .await
}
fn search() -> Value {
    json!({"kind":"find_dates","query":{"anchor":"2026-10-01","direction":"past","include_anchor":false,"limit":1,"within_days":146097,"months":[2],"month_days":[],"weekdays":[5],"leap_year":true,"month_end":true}})
}
fn calc(expression: &str) -> Value {
    json!({"kind":"calculate","expression":expression,"variables":[]})
}
fn calendar(
    operation: &str,
    date: Value,
    start: Value,
    end: Value,
    amount: Value,
    unit: Value,
) -> Value {
    json!({"kind":"calendar","operation":operation,"date":date,"start":start,"end":end,"amount":amount,"unit":unit})
}
#[tokio::test]
async fn dependent_dates_and_math_execute_in_one_plan_and_preserve_evidence() {
    let args = json!({"steps":[
        {"id":"span","action":calendar("diff",Value::Null,json!("2026-10-01"),json!("2026-10-15"),Value::Null,Value::Null)},
        {"id":"cost","action":{"kind":"calculate","expression":"days * rate","variables":[{"name":"days","value":{"ref":"/span/days"}},{"name":"rate","value":120}]}},
        {"id":"weeks","action":calc("14/7")},
        {"id":"end","action":calendar("add",json!("2026-10-01"),Value::Null,Value::Null,json!({"ref":"/weeks/value"}),json!("weeks"))}
    ]});
    assert_eq!(registry().work_units("compute", &args), 4);
    let r = run(args).await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["data"]["steps"][1]["data"]["value"], 1680.0);
    assert_eq!(
        r["data"]["steps"][1]["resolved_action"]["variables"][0]["value"],
        14
    );
    assert_eq!(r["data"]["steps"][3]["data"]["date"], "2026-10-15");
}
#[tokio::test]
async fn search_checks_intermediate_candidates_and_other_calendar_conditions() {
    let r = run(json!({"steps":[{"id":"match","action":search()}]})).await;
    assert_eq!(r["ok"], true, "{r}");
    let result = &r["data"]["steps"][0]["data"];
    assert_eq!(result["dates"][0]["date"], "2008-02-29");
    assert_eq!(result["checked_dates"], 6789);
    assert_eq!(result["range_exhausted"], false);
    let mut action = search();
    action["query"]["direction"] = json!("future");
    action["query"]["months"] = json!([]);
    action["query"]["leap_year"] = Value::Null;
    action["query"]["limit"] = json!(3);
    action["query"]["within_days"] = json!(500);
    let r = run(json!({"steps":[{"id":"months","action":action}]})).await;
    let dates = r["data"]["steps"][0]["data"]["dates"].as_array().unwrap();
    assert_eq!(
        dates
            .iter()
            .map(|v| v["date"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["2027-04-30", "2027-12-31"]
            .map(|s| s.to_string())
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
}
#[tokio::test]
async fn nearest_preserves_ties_and_empty_bounded_search_does_not_claim_global_impossibility() {
    let mut action = search();
    action["query"] = json!({"anchor":"2026-10-07","direction":"nearest","include_anchor":false,"limit":1,"within_days":30,"months":[],"month_days":[],"weekdays":[1,5],"leap_year":null,"month_end":null});
    let r = run(json!({"steps":[{"id":"tie","action":action}]})).await;
    let d = &r["data"]["steps"][0]["data"];
    assert_eq!(d["tie_at_limit"], true);
    assert_eq!(d["dates"][0]["date"], "2026-10-05");
    assert_eq!(d["dates"][1]["date"], "2026-10-09");
    let mut impossible = search();
    impossible["query"]["month_days"] = json!([30]);
    impossible["query"]["within_days"] = json!(40);
    let r = run(json!({"steps":[{"id":"empty","action":impossible}]})).await;
    assert_eq!(r["data"]["steps"][0]["data"]["dates"], json!([]));
    assert_eq!(r["data"]["steps"][0]["data"]["range_exhausted"], true);
    assert_eq!(r["data"]["steps"][0]["data"]["within_days"], 40);
}
#[tokio::test]
async fn partial_failure_stops_dependencies_and_preserves_successes() {
    let r=run(json!({"steps":[{"id":"a","action":calc("2+2")},{"id":"b","action":calc("1/0")},{"id":"c","action":calc("3+3")}]})).await;
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "PLAN_FAILED");
    assert_eq!(r["data"]["steps"][0]["data"]["value"], 4.0);
    assert_eq!(r["data"]["failed_step"], "b");
    assert_eq!(r["data"]["skipped_steps"], json!(["c"]));
    let r=run(json!({"steps":[{"id":"a","action":{"kind":"calculate","expression":"x+1","variables":[{"name":"x","value":{"ref":"/future/value"}}]}}]})).await;
    assert_eq!(r["ok"], false);
    assert!(
        r["data"]["steps"][0]["error"]
            .as_str()
            .unwrap()
            .contains("引用")
    );
}
#[tokio::test]
async fn rejects_invalid_dates_fractional_offsets_and_conflicting_parameters() {
    for action in [
        calendar(
            "inspect",
            json!("2100-02-29"),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ),
        calendar(
            "add",
            json!("2026-01-31"),
            Value::Null,
            Value::Null,
            json!(1),
            json!("months"),
        ),
        calendar(
            "today",
            json!("2020-01-01"),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ),
    ] {
        let r = run(json!({"steps":[{"id":"bad","action":action}]})).await;
        assert_eq!(r["ok"], false, "{r}");
    }
    let r=run(json!({"steps":[{"id":"half","action":calc("1/2")},{"id":"date","action":calendar("add",json!("2026-10-01"),Value::Null,Value::Null,json!({"ref":"/half/value"}),json!("days"))}]})).await;
    assert_eq!(r["data"]["failed_step"], "date");
}
#[tokio::test]
async fn plan_limits_schema_and_snapshot_are_enforced() {
    let args = json!({"steps":(0..17).map(|i|json!({"id":format!("s{i}"),"action":calc("2+2")})).collect::<Vec<_>>()});
    assert_eq!(run(args).await["error"]["code"], "INVALID_ARGUMENTS");
    let args =
        json!({"steps":[{"id":"same","action":calc("2+2")},{"id":"same","action":calc("3+3")}]});
    assert_eq!(run(args).await["error"]["code"], "TOOL_ERROR");
    let mut registry = registry();
    let snapshot = registry.clone();
    registry.disable("compute");
    let args = json!({"steps":[{"id":"a","action":calc("2+2")}]});
    assert_eq!(
        registry
            .execute("compute", args.clone(), &context(), Duration::from_secs(1))
            .await["ok"],
        false
    );
    assert_eq!(
        snapshot
            .execute("compute", args, &context(), Duration::from_secs(1))
            .await["ok"],
        true
    );
}
#[tokio::test]
async fn calendar_search_yields_to_timeouts() {
    let mut query = search();
    query["query"]["month_days"] = json!([30]);
    let r = registry()
        .execute(
            "compute",
            json!({"steps":[{"id":"all","action":query}]}),
            &context(),
            Duration::from_millis(1),
        )
        .await;
    assert_eq!(r["error"]["code"], "TOOL_TIMEOUT");
}
