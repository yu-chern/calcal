use super::*;
use crate::tools::ToolRegistry;
use std::path::Path;
fn context(text: &str) -> ToolContext {
    ToolContext {
        reference_time: "2026-10-01T22:30:00Z".parse().unwrap(),
        timezone: chrono_tz::Europe::Berlin,
        sources: vec![text.into()],
        selections: BTreeMap::new(),
        verified_answers: BTreeMap::new(),
    }
}
fn input(id: &str, quote: &str, kind: &str) -> Value {
    json!({"id":id,"source":0,"quote":quote,"kind":kind})
}
fn step(id: &str, expression: &str) -> Value {
    json!({"id":id,"expression":expression})
}
fn plan(inputs: Vec<Value>, steps: Vec<Value>, refs: &[&str]) -> Value {
    json!({"inputs":inputs,"steps":steps,"outputs":refs.iter().map(|r|json!({"ref":r,"label":"result","unit":"none"})).collect::<Vec<_>>()})
}
async fn run(text: &str, args: Value) -> Value {
    let config =
        crate::agent::config::SystemConfig::load(Path::new("system_config/config.toml")).unwrap();
    ToolRegistry::load(Path::new("system_config"), &config.tools.definitions)
        .unwrap()
        .execute("compute", args, &context(text), Duration::from_secs(2))
        .await
}
async fn expression(s: &str) -> Value {
    run(
        s,
        plan(
            vec![input("x", s, "expression")],
            vec![step("answer", "x")],
            &["answer"],
        ),
    )
    .await
}
#[tokio::test]
async fn exact_math_preserves_large_integers_decimals_fractions_and_rounding() {
    for (expr, expected) in [
        ("9007199254740993+1", "9007199254740994"),
        ("0.1+0.2", "0.3"),
        ("1/3+1/6", "0.5"),
        ("1/3", "1/3"),
        ("-2^2", "-4"),
        ("2^3^2", "512"),
        ("sqrt(144)", "12"),
        ("sqrt(1/4)", "0.5"),
        ("round_places(2.675,2)", "2.68"),
        ("round_places(-2.675,2)", "-2.68"),
        ("floor(-1.5)", "-2"),
        ("ceil(-1.5)", "-1"),
        ("1e-3*1e3", "1"),
        ("0/7", "0"),
        ("(-4)^3", "-64"),
        ("2^-3", "0.125"),
    ] {
        let r = expression(expr).await;
        assert_eq!(r["ok"], true, "{expr}: {r}");
        assert_eq!(r["data"]["claims"][0]["data"]["value"], expected, "{expr}");
    }
    let r = expression("sqrt(2)").await;
    assert_eq!(r["data"]["claims"][0]["data"]["exact"], false);
    assert!(r["data"]["text"].as_str().unwrap().contains("约"));
}
#[tokio::test]
async fn combined_dates_math_uses_only_bound_inputs_and_step_dependencies() {
    let r = run(
        "2026-10-01到2026-10-15每天120元",
        plan(
            vec![
                input("start", "2026-10-01", "date"),
                input("end", "2026-10-15", "date"),
                input("rate", "120", "number"),
            ],
            vec![
                step("span", "date_diff(start,end)"),
                step("cost", "span*rate"),
            ],
            &["span", "cost"],
        ),
    )
    .await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["data"]["claims"][1]["data"]["value"], "1680");
    assert_eq!(
        r["data"]["steps"][1]["dependencies"],
        json!(["rate", "span"])
    );
    assert_eq!(r["data"]["inputs"][0]["quote"], "2026-10-01");
}
#[tokio::test]
async fn date_search_is_complete_bounded_and_preserves_nearest_ties() {
    let r = run(
        "2026-10-01过去闰年二月最后一天为星期五",
        plan(
            vec![
                input("anchor", "2026-10-01", "date"),
                input("m", "二月", "month"),
                input("w", "星期五", "weekday"),
                input("direction", "过去", "direction"),
            ],
            vec![step(
                "found",
                "find_dates(anchor,direction,one,cycle_days,[m],[],[w],true,true,false)",
            )],
            &["found"],
        ),
    )
    .await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        r["data"]["claims"][0]["data"]["dates"][0]["date"],
        "2008-02-29"
    );
    assert_eq!(r["data"]["search_evidence"][0]["checked_dates"], 6789);
    let inputs = vec![
        input("a", "2026-10-07", "date"),
        input("mon", "周一", "weekday"),
        input("fri", "周五", "weekday"),
        input("direction", "双向", "direction"),
    ];
    let search = step(
        "found",
        "find_dates(a,direction,one,cycle_days,[],[],[mon,fri],any,any,false)",
    );
    let r = run(
        "2026-10-07 周一 周五 双向",
        plan(inputs.clone(), vec![search.clone()], &["found"]),
    )
    .await;
    assert_eq!(
        r["data"]["claims"][0]["data"]["dates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(r["data"]["text"].as_str().unwrap().contains("并列"));
    let r = run(
        "2026-10-07 周一 周五 双向",
        plan(
            inputs,
            vec![search, step("arbitrary", "at(dates(found),zero)")],
            &["arbitrary"],
        ),
    )
    .await;
    assert_eq!(r["ok"], false);
}
#[tokio::test]
async fn list_filter_map_and_aggregation_run_inside_the_tool() {
    let r = run(
        "2026-10-01至2026-10-31，周五每天120元",
        plan(
            vec![
                input("a", "2026-10-01", "date"),
                input("b", "2026-10-31", "date"),
                input("w", "周五", "weekday"),
                input("rate", "120", "number"),
            ],
            vec![
                step("selected", "filter(range(a,b),weekday(item)==w)"),
                step("total", "count(selected)*rate"),
            ],
            &["total"],
        ),
    )
    .await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["data"]["claims"][0]["data"]["value"], "600");
    let r = expression("sum(map(range(1,5),item^2))").await;
    assert_eq!(r["data"]["claims"][0]["data"]["value"], "55");
    let r = expression("mean(sort([9,1,5]))").await;
    assert_eq!(r["data"]["claims"][0]["data"]["value"], "5");
}
#[tokio::test]
async fn rejects_fabricated_inputs_literal_smuggling_forward_refs_and_answer_injection() {
    for args in [
        plan(
            vec![input("days", "14", "number")],
            vec![step("cost", "days")],
            &["cost"],
        ),
        plan(vec![], vec![step("cost", "14*120")], &["cost"]),
        plan(vec![], vec![step("cost", "1680")], &["cost"]),
        plan(vec![], vec![step("cost", "missing*one")], &["cost"]),
        plan(
            vec![input("user", "120", "number")],
            vec![step("cost", "user")],
            &["user"],
        ),
        json!({"inputs":[],"steps":[{"id":"cost","expression":"one"}],"outputs":[{"ref":"cost","label":"结果是1680","unit":"none"}]}),
        plan(vec![], vec![step("cost", "today.__class__")], &["cost"]),
        plan(vec![], vec![step("cost", "open(one)")], &["cost"]),
        plan(
            vec![input("one", "120", "number")],
            vec![step("cost", "one")],
            &["cost"],
        ),
    ] {
        let r = run("2014年，每天120元", args).await;
        assert_eq!(r["ok"], false, "{r}");
    }
    let mut args = plan(
        vec![input("x", "120", "number")],
        vec![step("cost", "x")],
        &["cost"],
    );
    args["inputs"][0]["source"] = json!(1);
    assert_eq!(run("120", args).await["ok"], false);
}
#[tokio::test]
async fn domain_overflow_and_resource_errors_fail_closed() {
    for expr in ["10^100", "range(1,5000)", "round_places(1,100)", "1e100"] {
        let r = expression(expr).await;
        assert_eq!(r["ok"], false, "{expr}: {r}");
        assert!(r["data"]["text"].is_null());
    }
    let r = run(
        "2026-01-31",
        plan(
            vec![input("a", "2026-01-31", "date")],
            vec![
                step("bad", "date_add(a,one,months)"),
                step("later", "today"),
            ],
            &["later"],
        ),
    )
    .await;
    assert_eq!(r["ok"], false);
    assert_eq!(r["data"]["skipped_steps"], json!(["later"]));
    let r = run(
        "2026-01-31 月末",
        plan(
            vec![
                input("a", "2026-01-31", "date"),
                input("rule", "月末", "month_rule"),
            ],
            vec![step("end", "date_add(a,one,months,rule)")],
            &["end"],
        ),
    )
    .await;
    assert_eq!(r["data"]["claims"][0]["data"]["date"], "2026-02-28");
}
#[tokio::test]
async fn current_date_is_server_supplied_and_chinese_inputs_are_normalized_by_tool() {
    let r = run(
        "今天",
        plan(vec![], vec![step("answer", "today")], &["answer"]),
    )
    .await;
    assert_eq!(r["data"]["claims"][0]["data"]["date"], "2026-10-02");
    for (quote, value) in [
        ("八折", "0.8"),
        ("百分之二十", "0.2"),
        ("半", "0.5"),
        ("负十二", "-12"),
        ("一百二十三", "123"),
    ] {
        let r = run(
            quote,
            plan(
                vec![input("x", quote, "number")],
                vec![step("answer", "x")],
                &["answer"],
            ),
        )
        .await;
        assert_eq!(r["data"]["claims"][0]["data"]["value"], value, "{r}");
    }
}
#[tokio::test]
async fn clarification_templates_cannot_introduce_computed_prose() {
    use super::super::clarification_v2::Clarification;
    let ctx = context("最近是哪一天");
    let r = Clarification
        .execute(json!({"topic":"nearest","source":null,"quote":null}), &ctx)
        .await
        .unwrap();
    assert!(r["text"].as_str().unwrap().contains("只向过去"));
    assert!(
        Clarification
            .execute(json!({"topic":"nearest","source":0,"quote":"2036年"}), &ctx)
            .await
            .is_err()
    );
    assert!(
        Clarification
            .execute(
                json!({"topic":"nearest","source":null,"quote":null,"question":"2036年是答案"}),
                &ctx
            )
            .await
            .is_err()
    );
}
#[test]
fn legacy_tools_cannot_be_loaded_alongside_verified_runtime() {
    assert!(
        ToolRegistry::load(
            Path::new("system_config"),
            &[
                "tools/compute.v2.json".into(),
                "tools/calculator.v1.json".into()
            ]
        )
        .is_err()
    );
}
#[test]
fn rational_operations_match_small_integer_identities() {
    for a in -20..=20 {
        for b in 1..=20 {
            for c in -10..=10 {
                let x = Number::rational(a, b).unwrap();
                let y = Number::integer(c);
                assert_eq!(
                    x.add(&y).unwrap().add(&y.neg().unwrap()).unwrap().text(),
                    x.text()
                );
                if c != 0 {
                    assert_eq!(x.mul(&y).unwrap().div(&y).unwrap().text(), x.text());
                }
            }
        }
    }
}

#[tokio::test]
async fn source_normalization_rejects_truncated_sign_exponent_percentage_and_ambiguous_chinese() {
    for (message, quote) in [
        ("-5", "5"),
        ("1e3", "3"),
        ("1e3", "1"),
        ("20%", "20"),
        ("一百二", "一百二"),
        ("一万二", "一万二"),
        ("十十", "十十"),
    ] {
        let r = run(
            message,
            plan(
                vec![input("x", quote, "number")],
                vec![step("answer", "x")],
                &["answer"],
            ),
        )
        .await;
        assert_eq!(r["ok"], false, "{message}: {r}");
    }
    for (quote, value) in [
        ("一万亿", "1000000000000"),
        ("一亿零三万", "100030000"),
        ("一点五", "1.5"),
        ("一百零二", "102"),
        ("负一点五", "-1.5"),
    ] {
        let r = run(
            quote,
            plan(
                vec![input("x", quote, "number")],
                vec![step("answer", "x")],
                &["answer"],
            ),
        )
        .await;
        assert_eq!(r["data"]["claims"][0]["data"]["value"], value, "{r}");
    }
}

#[tokio::test]
async fn undefined_math_is_a_verified_non_numeric_conclusion() {
    for expr in ["1/0", "sqrt(-1)", "mean([])", "date(2025,2,29)"] {
        let r = expression(expr).await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["data"]["claims"][0]["data"]["defined"], false);
        assert!(r["data"]["text"].as_str().unwrap().contains("无法得到"));
    }
}
#[tokio::test]
async fn search_direction_cannot_be_invented_and_numbered_choices_are_resolved_by_server() {
    let r = run(
        "最近",
        plan(
            vec![input("dir", "最近", "direction")],
            vec![step("answer", "dir")],
            &["answer"],
        ),
    )
    .await;
    assert_eq!(r["ok"], false);
    let r = run(
        "最近",
        plan(
            vec![],
            vec![step(
                "answer",
                "find_dates(today,nearest,one,cycle_days,[],[],[],any,any,false)",
            )],
            &["answer"],
        ),
    )
    .await;
    assert_eq!(r["ok"], false);
    let mut ctx = context("3");
    ctx.selections.insert(0, "nearest".into());
    let args = plan(
        vec![input("direction", "3", "direction")],
        vec![step(
            "found",
            "find_dates(today,direction,one,cycle_days,[],[],[],any,any,false)",
        )],
        &["found"],
    );
    let r = Program.execute(args, &ctx).await.unwrap();
    assert_eq!(r["complete"], true);
    assert_eq!(r["search_evidence"][0]["direction"], "nearest");
}

#[tokio::test]
async fn arithmetic_operands_list_sources_and_confirmed_rules_are_not_rejected() {
    let r = run(
        "解方程2*x+3=11",
        plan(
            vec![
                input("a", "2", "number"),
                input("b", "3", "number"),
                input("rhs", "11", "number"),
            ],
            vec![step("answer", "(rhs-b)/a")],
            &["answer"],
        ),
    )
    .await;
    assert_eq!(r["data"]["claims"][0]["data"]["value"], "4", "{r}");
    let r = run(
        "1、2、3、4、5",
        plan(
            vec![input("values", "1、2、3、4、5", "numbers")],
            vec![step("answer", "sum(map(values,square(item)))")],
            &["answer"],
        ),
    )
    .await;
    assert_eq!(r["data"]["claims"][0]["data"]["value"], "55", "{r}");
    let r = run(
        "2026年1月31日加一个月，若该日不存在就用目标月末日期",
        plan(
            vec![
                input("start", "2026年1月31日", "date"),
                input("amount", "一个月", "number"),
                input("rule", "若该日不存在就用目标月末日期", "month_rule"),
            ],
            vec![step("answer", "date_add(start,amount,months,rule)")],
            &["answer"],
        ),
    )
    .await;
    assert_eq!(r["data"]["claims"][0]["data"]["date"], "2026-02-28", "{r}");
}

#[tokio::test]
async fn numbered_choice_requires_the_complete_server_question_not_quoted_text() {
    use crate::tools::{clarification_selection, clarification_v2::Clarification};
    assert!(
        clarification_selection("你提供的条件：最近。“最近”采用哪种方向和距离口径？", "3")
            .is_none()
    );
    let q = Clarification
        .execute(
            json!({"topic":"nearest","source":null,"quote":null}),
            &context("最近"),
        )
        .await
        .unwrap();
    assert_eq!(
        clarification_selection(q["text"].as_str().unwrap(), "3"),
        Some("nearest")
    );
}
