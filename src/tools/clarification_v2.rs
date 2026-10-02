//! Questions use fixed templates; untrusted computed claims cannot enter prose.
use super::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
pub struct Clarification;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    topic: String,
    source: Option<usize>,
    quote: Option<String>,
}
#[async_trait]
impl Tool for Clarification {
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Value, String> {
        let q: Question = serde_json::from_value(args).map_err(|_| "澄清参数无效")?;
        let (question, options): (&str, Vec<&str>) = match q.topic.as_str() {
            "nearest" => (
                "“最近”采用哪种方向和距离口径？",
                vec![
                    "只向过去找",
                    "只向未来找",
                    "前后比较，按实际日期相差天数取最近",
                ],
            ),
            "distance" => (
                "你希望按哪种距离比较？",
                vec!["实际日期相差天数", "日历年份之差"],
            ),
            "month_end" => (
                "月份或年份偏移可能遇到不存在的日期，请确认处理规则。",
                vec![
                    "使用目标月份的最后一天",
                    "超出的天数向后顺延",
                    "保留原日期要求，遇到不存在的日期就说明无法得到该日期",
                ],
            ),
            "endpoints" => (
                "计算天数采用哪种首尾口径？",
                vec!["结束日期减开始日期", "首尾两天都计入", "首尾两天都不计入"],
            ),
            "range" => ("请补充明确的开始日期、结束日期或搜索范围。", vec![]),
            "conflicting" => ("这些条件存在冲突，请说明应保留或修改哪些条件。", vec![]),
            "units" => ("请确认数值的单位及需要转换成的单位。", vec![]),
            "rate" => (
                "请确认比例或利率的基数、周期，以及需要使用的计算规则。",
                vec![],
            ),
            "rounding" => (
                "请确认保留位数及舍入规则。",
                vec!["四舍五入，恰好一半时远离零", "向下取整", "向上取整"],
            ),
            "precision" => (
                "该任务超出当前精确计算范围，请确认是否接受近似计算，或缩小数值范围。",
                vec!["接受标明精度的近似值", "保留精确要求，我会补充或调整问题"],
            ),
            "timezone" => (
                "请确认使用的时区，以及需要日历日期差还是实际经过时长。",
                vec![],
            ),
            "tie" => (
                "搜索出现同距离的并列日期，请确认如何处理。",
                vec!["优先过去的日期", "优先未来的日期", "保留全部并列结果"],
            ),
            "conditions" => (
                "请补充或明确需要采用的条件及计算规则，以便继续计算。",
                vec![],
            ),
            _ => return Err("未知澄清主题".into()),
        };
        let mut text = String::new();
        match (q.source, q.quote.as_deref()) {
            (Some(index), Some(quote))
                if !quote.is_empty()
                    && quote.chars().count() <= 300
                    && ctx.sources.get(index).is_some_and(|s| s.contains(quote)) =>
            {
                text.push_str(&format!("你提供的条件：「{quote}」\n"));
            }
            (None, None) => {}
            _ => return Err("澄清引用必须完整匹配用户原文，不能写入模型推导的结论".into()),
        }
        text.push_str(question);
        for (i, option) in options.iter().enumerate() {
            text.push_str(&format!("\n{}. {}", i + 1, option));
        }
        Ok(
            json!({"topic":q.topic,"question":question,"options":options,"source":q.source,"quote":q.quote,"text":text}),
        )
    }
}

/// Resolve only an explicit numbered reply to a known server-rendered question.
pub fn selection(question: &str, reply: &str) -> Option<&'static str> {
    let reply = reply
        .trim()
        .trim_end_matches(['。', '.'])
        .trim_start_matches("选");
    let index = match reply {
        "1" | "第一项" | "第一个" => 0,
        "2" | "第二项" | "第二个" => 1,
        "3" | "第三项" | "第三个" => 2,
        _ => return None,
    };
    if question.ends_with("“最近”采用哪种方向和距离口径？\n1. 只向过去找\n2. 只向未来找\n3. 前后比较，按实际日期相差天数取最近") {
        Some(["past", "future", "nearest"][index])
    } else if question.ends_with("月份或年份偏移可能遇到不存在的日期，请确认处理规则。\n1. 使用目标月份的最后一天\n2. 超出的天数向后顺延\n3. 保留原日期要求，遇到不存在的日期就说明无法得到该日期")
    {
        Some(["clamp", "carry", "strict"][index])
    } else {
        None
    }
}
