//! Final-response protocol: reviewed concepts, verified computation, or verified history.
//! There is deliberately no model-authored answer string in the arguments.
use super::{Tool, ToolContext, program::Program};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub struct Reply;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    topics: Vec<String>,
    program: Option<Value>,
    reference: Option<String>,
}
fn entry(id: &str) -> Result<Value, String> {
    let text = match id {
        "greeting" => {
            "你好！我可以帮你查询公历日期、进行常见数学运算，也可以解释相关概念。你想了解什么？"
        }
        "capabilities" => {
            "我可以查询日期和星期、计算日期差与日期偏移、搜索符合条件的日期，进行常见数学运算，以及把日期和计算组合起来。也可以解释闰年、百分比、舍入等概念。有会影响答案的歧义时，我会先确认。具体计算会交给工具执行。"
        }
        "thanks" => "不客气！有其他日期或计算问题，随时告诉我。",
        "farewell" => "再见！需要日期查询或计算时，欢迎继续来问。",
        "scope" => {
            "我主要帮助处理公历日期、常见数学运算及相关概念。这项请求超出了当前服务范围；你也可以换一个日期或计算问题。"
        }
        "concept_help" => {
            "我目前可以解释闰年、公历与农历的区别、日期差与首尾口径、月末偏移、时区和夏令时、工作日、百分比、平均值、运算顺序、舍入和计算精度。你可以选择一个主题继续问。"
        }
        "leap_year" => {
            "闰年是在历法中增加一天或一个月，以协调历法与天文周期的年份。在公历中，增加的那一天放在二月。公历判定规则是：年份能被4整除且不能被100整除，或者能被400整除。判断某个具体年份是否满足规则，需要执行日期工具。"
        }
        "gregorian_lunar" => {
            "公历属于太阳历，年份与季节周期相协调。中国农历属于阴阳合历：月份依据月相周期，年份通过闰月等规则与季节协调，所以它并非单纯的阴历。我可以解释这些概念；具体农历日期转换或节日日期查询需要相应的历法数据，当前工具未提供。"
        }
        "date_difference" => {
            "日期差通常指结束日期减去开始日期，表示相隔的日历天数。它与把日期逐个计入的天数不同；如果是计费或统计，需要明确是否计入首尾日期。跨时区或夏令时变化时，日历日期差也不能直接当成实际经过的小时数。"
        }
        "inclusive_dates" => {
            "“包含首尾”表示开始日期和结束日期都计入统计；“相差多少天”通常按结束日期减开始日期。计费等任务需要明确这两种口径，不能把它们混用。具体天数和费用应由工具计算。"
        }
        "month_shift" => {
            "月份长短不同，给日期增加月份或年份时，目标月份可能没有原来的那一天。需要明确是改用目标月末、把超出的天数向后顺延，还是保留原日并说明无法得到该日期。没有确认规则时，我会先澄清。"
        }
        "timezone_dst" => {
            "同一个时刻在不同时区可能对应不同的当地日期。夏令时会调整当地时钟，因此按日历数天与计算实际经过时长是不同任务。当前工具处理配置时区下的日期，不提供任意时区之间的实际经过小时数计算。"
        }
        "workdays" => {
            "“工作日”可能指周一至周五，也可能指按当地节假日和调休安排确定的工作日期。二者不能直接等同。当前没有地区节假日数据库；如果你提供日历，或明确只按星期规则筛选，就可以按该口径计算。"
        }
        "nearest_date" => {
            "“最近”需要明确是只向过去找、只向未来找，还是前后比较。前后比较还需要确定距离口径，例如实际日期相差天数或日历年份之差。出现并列结果时，也应明确取舍规则。"
        }
        "percentages" => {
            "百分比表达某个量相对于基数的比例。计算折扣、增长率或占比时，要先明确基数；比例相同但基数不同，计算任务也不同。具体百分比换算和结果由工具计算。"
        }
        "average" => {
            "算术平均值是各个数值之和除以数值个数。加权平均值则要把各个数值与对应权重结合起来，并按权重总量计算。空集合没有算术平均值；遇到缺失数据时，需要先明确处理规则。"
        }
        "rounding" => {
            "舍入需要同时确定保留位数和规则。常见规则包括四舍五入、向下取整和向上取整。恰好在中点时，不同的四舍五入约定也可能不同；本工具采用中点远离零。计算过程中何时舍入也应明确。"
        }
        "precision" => {
            "精确计算与近似计算的区别在于数值如何表示和执行。当前工具对有范围限制的整数、有限小数和分数使用精确有理数运算；非终止小数可以保留为分数。部分平方根、三角函数、指数和对数采用明确标记的近似值。超出精确范围时会报错，不会默默改成近似值。"
        }
        "operation_order" => {
            "数学表达式通过括号明确分组，并按约定的运算优先级执行。同一级运算还需要遵循结合规则。文字描述可能有多种分组方式时，应先确认；具体表达式的求值由工具完成。"
        }
        "division_zero" => {
            "除法需要存在能满足相应乘法关系的商。除数为零时，通常的实数除法无法确定一个唯一有效的商，因此不能作为普通数值运算执行。具体表达式是否无定义会由工具验证。"
        }
        "units" => {
            "数值需要与单位一起理解。相加、比较或换算前，要确认量的种类和单位口径。计费还需要明确每单位价格、统计范围和舍入规则。具体换算和费用由工具执行。"
        }
        "simple_compound_interest" => {
            "单利按原始本金作为计息基数；复利把已计入的利息纳入后续计息基数。实际计算需要明确本金、利率周期、计息次数及舍入口径；这些条件齐全后再用工具计算。"
        }
        "calculation_process" => {
            "我先理解你的条件，把会影响答案的歧义问清，再把明确的任务交给计算工具。派生数值、日期、筛选和比较由工具执行，最终计算结论来自成功结果。模型选择公式和条件仍可能理解错误，所以条件和口径需要说清楚。"
        }
        _ => return Err("知识主题不存在，不能用主题字段填写计算结论".into()),
    };
    let sources: &[&str] = match id {
        "leap_year" | "gregorian_lunar" => &["https://aa.usno.navy.mil/faq/calendars"],
        _ => &[],
    };
    Ok(json!({"id":id,"version":"1","text":text,"sources":sources}))
}
fn explain(reference: &Value) -> Result<String, String> {
    if reference["program"]["complete"] != true
        || reference["call_id"].as_str().is_none()
        || reference["text"].as_str().is_none()
        || reference["claims"].as_array().is_none_or(Vec::is_empty)
    {
        return Err("引用没有完整的成功计算证据".into());
    }
    let mut lines = vec!["这段说明引用此前已成功执行的计算结果。".to_owned()];
    for s in reference["program"]["steps"]
        .as_array()
        .ok_or("引用缺少计算步骤")?
    {
        if s["ok"] != true {
            return Err("不能解释失败步骤为成功计算".into());
        }
        let expression = s["expression"].as_str().ok_or("引用缺少计算表达式")?;
        let operation = if expression.starts_with("find_dates(") {
            "按已确认的方向和条件搜索日期"
        } else if expression.starts_with("date_diff(") {
            "用结束日期减开始日期求日期差"
        } else if expression.starts_with("date_add(") {
            "按已确认的偏移规则计算日期"
        } else if expression.starts_with("filter(") {
            "按条件筛选"
        } else if expression.starts_with("map(") {
            "对列表逐项执行计算"
        } else if expression.contains("count(") {
            "统计数量并执行后续运算"
        } else if expression.starts_with("sum(") {
            "求和"
        } else if expression.starts_with("mean(") {
            "求平均值"
        } else if expression.contains('*') {
            "执行乘法"
        } else if expression.contains('/') {
            "执行除法"
        } else {
            "执行已记录的计算步骤"
        };
        lines.push(format!("步骤{}：{}。", lines.len(), operation));
    }
    lines.push(format!(
        "已验证的结果：\n{}",
        reference["text"].as_str().unwrap()
    ));
    Ok(lines.join("\n"))
}
#[async_trait]
impl Tool for Reply {
    fn work_units(&self, args: &Value) -> usize {
        args["program"]["steps"]
            .as_array()
            .map_or(1, |s| s.len().saturating_add(1))
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Value, String> {
        let request: Request = serde_json::from_value(args).map_err(|_| "回答参数无效")?;
        if request.topics.len() > 4 || request.program.is_some() && request.reference.is_some() {
            return Err("最多4个知识主题；本轮计算和历史引用不能同时提交".into());
        }
        let mut seen = BTreeSet::new();
        let mut knowledge = vec![];
        for topic in &request.topics {
            if !seen.insert(topic) {
                return Err("知识主题不能重复".into());
            }
            knowledge.push(entry(topic)?);
        }
        if request.program.is_none() && request.reference.is_none() && knowledge.is_empty() {
            return Err("回答必须选择知识主题、计算程序或已验证引用".into());
        }
        let mut result = if let Some(program) = request.program {
            let result = Program.execute(program, ctx).await?;
            if result["complete"] != true {
                return Ok(result);
            }
            result
        } else {
            json!({"complete":true,"claims":[]})
        };
        let mut lines: Vec<String> = knowledge
            .iter()
            .map(|e| e["text"].as_str().unwrap().into())
            .collect();
        let kind = if let Some(key) = request.reference {
            let reference = ctx
                .verified_answers
                .get(&key)
                .ok_or("引用不存在、不属于当前会话或不是成功的计算答案")?;
            lines.push(explain(reference)?);
            result["reference"] = json!({"key":key,"run_id":reference["run_id"],"call_id":reference["call_id"],"claims":reference["claims"]});
            "explanation"
        } else if let Some(text) = result["text"].as_str() {
            lines.push(text.into());
            "computed"
        } else {
            "knowledge"
        };
        result["response_kind"] = json!(kind);
        result["knowledge"] = json!(knowledge);
        result["text"] = json!(lines.join("\n\n"));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;
    use std::{path::Path, time::Duration};
    fn ctx() -> ToolContext {
        ToolContext {
            reference_time: "2026-10-02T06:00:00Z".parse().unwrap(),
            timezone: chrono_tz::Europe::Berlin,
            sources: vec!["什么是闰年？2024年是否是闰年？".into()],
            selections: Default::default(),
            verified_answers: Default::default(),
        }
    }
    async fn run(args: Value) -> Value {
        let cfg = crate::agent::config::SystemConfig::load(Path::new("system_config/config.toml"))
            .unwrap();
        ToolRegistry::load(Path::new("system_config"), &cfg.tools.definitions)
            .unwrap()
            .execute("respond", args, &ctx(), Duration::from_secs(2))
            .await
    }
    #[tokio::test]
    async fn knowledge_replies_are_closed_templates_and_terminal() {
        let r = run(json!({"topics":["greeting"],"program":null,"reference":null})).await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["data"]["response_kind"], "knowledge");
        assert_eq!(r["data"]["claims"], json!([]));
        assert!(r["data"]["text"].as_str().unwrap().contains("你好"));
        for args in [
            json!({"topics":["2036年是答案"],"program":null,"reference":null}),
            json!({"topics":["greeting"],"program":null,"reference":null,"text":"日期是2036年"}),
            json!({"topics":[],"program":null,"reference":"another-conversation"}),
            json!({"topics":[],"program":null,"reference":null}),
        ] {
            assert_eq!(run(args).await["ok"], false);
        }
    }
    #[tokio::test]
    async fn mixed_concept_and_calculation_share_the_verified_runtime() {
        let p = json!({"inputs":[{"id":"y","source":0,"quote":"2024","kind":"number"}],"steps":[{"id":"d","expression":"date(y,one,one)"},{"id":"leap","expression":"is_leap(d)"}],"outputs":[{"ref":"leap","label":"comparison","unit":"none"}]});
        let r = run(json!({"topics":["leap_year"],"program":p,"reference":null})).await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["data"]["response_kind"], "computed");
        assert_eq!(r["data"]["claims"][0]["data"]["value"], true);
        let bad = json!({"inputs":[],"steps":[{"id":"fake","expression":"2024"}],"outputs":[{"ref":"fake","label":"result","unit":"none"}]});
        let r = run(json!({"topics":["leap_year"],"program":bad,"reference":null})).await;
        assert_eq!(r["ok"], false);
        assert!(r["data"]["text"].is_null());
    }
    #[tokio::test]
    async fn references_reject_incomplete_evidence_and_charge_nested_work() {
        let args = json!({"topics":[],"program":null,"reference":"known"});
        let mut context = ctx();
        let result=Program.execute(json!({"inputs":[{"id":"yr","source":0,"quote":"2024","kind":"number"}],"steps":[{"id":"answer","expression":"is_leap(date(yr,one,one))"}],"outputs":[{"ref":"answer","label":"comparison","unit":"none"}]}),&context).await.unwrap();
        let evidence = json!({"run_id":"previous","call_id":"tool","text":result["text"],"claims":result["claims"],"program":result});
        context
            .verified_answers
            .insert("known".into(), evidence.clone());
        let r = Reply.execute(args.clone(), &context).await.unwrap();
        assert_eq!(r["response_kind"], "explanation");
        assert_eq!(r["reference"]["claims"], evidence["claims"]);
        for path in ["complete", "step"] {
            let mut bad = evidence.clone();
            if path == "complete" {
                bad["program"]["complete"] = json!(false);
            } else {
                bad["program"]["steps"][0]["ok"] = json!(false);
            }
            context.verified_answers.insert("known".into(), bad);
            assert!(Reply.execute(args.clone(), &context).await.is_err());
        }
        assert_eq!(
            Reply.work_units(&json!({"program":{"steps":[{}, {}, {}]}})),
            4
        );
    }
    #[test]
    fn every_advertised_topic_has_reviewed_content() {
        let d: Value =
            serde_json::from_str(include_str!("../../system_config/tools/respond.v1.json"))
                .unwrap();
        for t in d["input_schema"]["properties"]["topics"]["items"]["enum"]
            .as_array()
            .unwrap()
        {
            let e = entry(t.as_str().unwrap()).unwrap();
            assert!(!e["text"].as_str().unwrap().is_empty());
        }
    }
}
