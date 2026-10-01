//! Provenance-bound, typed calculation programs and server-rendered answers.
mod functions;
mod number;
mod source;
mod syntax;
#[cfg(test)]
mod tests;

use super::{Tool, ToolContext};
use async_trait::async_trait;
use chrono::{Datelike, NaiveDate};
use number::Number;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use syntax::Expr;

pub struct Program;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    inputs: Vec<Input>,
    steps: Vec<Step>,
    outputs: Vec<Output>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
    source: usize,
    quote: String,
    kind: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    id: String,
    expression: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    r#ref: String,
    label: String,
    unit: String,
}
#[derive(Clone, Debug)]
enum Datum {
    Number(Number),
    Date(NaiveDate),
    Bool(bool),
    List(Vec<Datum>),
    Symbol(String),
    Any,
    Undefined(String),
    Search { dates: Vec<Datum>, evidence: Value },
}
impl Datum {
    fn number(&self) -> Result<&Number, String> {
        if let Self::Number(n) = self {
            Ok(n)
        } else {
            Err("需要数值类型".into())
        }
    }
    fn integer(&self) -> Result<i128, String> {
        self.number()?.int()
    }
    fn date(&self) -> Result<NaiveDate, String> {
        if let Self::Date(d) = self {
            Ok(*d)
        } else {
            Err("需要日期类型".into())
        }
    }
    fn boolean(&self) -> Result<bool, String> {
        if let Self::Bool(v) = self {
            Ok(*v)
        } else {
            Err("需要布尔条件".into())
        }
    }
    fn list(&self) -> Result<&[Datum], String> {
        if let Self::List(v) = self {
            Ok(v)
        } else {
            Err("需要列表类型".into())
        }
    }
    fn symbol(&self) -> Result<&str, String> {
        if let Self::Symbol(v) = self {
            Ok(v)
        } else {
            Err("需要规则枚举".into())
        }
    }
    fn json(&self) -> Value {
        match self {
            Self::Number(Number::Exact(n, d)) => {
                json!({"type":"number","value":Number::Exact(*n,*d).text(),"numerator":n.to_string(),"denominator":d.to_string(),"exact":true})
            }
            Self::Number(Number::Approx(v)) => {
                json!({"type":"number","value":v,"exact":false,"precision":"IEEE-754 binary64"})
            }
            Self::Date(d) => {
                json!({"type":"date","date":d.to_string(),"year":d.year(),"month":d.month(),"day":d.day(),"weekday":d.weekday().number_from_monday(),"leap_year":d.leap_year()})
            }
            Self::Bool(v) => json!({"type":"boolean","value":v}),
            Self::List(v) => {
                json!({"type":"list","items":v.iter().map(Self::json).collect::<Vec<_>>()})
            }
            Self::Search { dates, evidence } => {
                json!({"type":"date_search","dates":dates.iter().map(Self::json).collect::<Vec<_>>(),"evidence":evidence})
            }
            Self::Symbol(s) => json!({"type":"rule","value":s}),
            Self::Any => Value::Null,
            Self::Undefined(message) => {
                json!({"type":"undefined","defined":false,"reason":message})
            }
        }
    }
    fn text(&self) -> Result<String, String> {
        match self {
            Self::Number(n) => Ok(n.text()),
            Self::Undefined(message) => Ok(format!("无法得到数值：{message}。")),
            Self::Date(d) => Ok(format!(
                "{}（星期{}）",
                d,
                ["一", "二", "三", "四", "五", "六", "日"]
                    [d.weekday().num_days_from_monday() as usize]
            )),
            Self::Bool(v) => Ok(if *v { "是" } else { "否" }.into()),
            Self::List(v) => Ok(if v.is_empty() {
                "空列表".into()
            } else {
                v.iter()
                    .map(Self::text)
                    .collect::<Result<Vec<_>, _>>()?
                    .join("、")
            }),
            Self::Search { dates, evidence } => {
                let mut text = if dates.is_empty() {
                    "此搜索范围内没有符合条件的日期。".into()
                } else {
                    dates
                        .iter()
                        .map(Self::text)
                        .collect::<Result<Vec<_>, _>>()?
                        .join("、")
                };
                if evidence["tie_at_limit"] == true {
                    text.push_str("；存在同距离并列结果。");
                }
                if evidence["range_exhausted"] == true && !dates.is_empty() {
                    text.push_str("；搜索范围内未找到足够数量的结果。");
                }
                text.push_str(&format!(
                    "\n搜索口径：以 {} 为参考，{}，最多检查 {} 天，{}参考日。",
                    evidence["anchor"].as_str().unwrap_or(""),
                    match evidence["direction"].as_str() {
                        Some("past") => "向过去",
                        Some("future") => "向未来",
                        _ => "前后按实际天数距离比较",
                    },
                    evidence["within_days"],
                    if evidence["include_anchor"] == true {
                        "包含"
                    } else {
                        "不包含"
                    }
                ));
                Ok(text)
            }
            _ => Err("规则常量不能作为计算结论输出".into()),
        }
    }
}
struct Engine {
    env: BTreeMap<String, Datum>,
    today: NaiveDate,
    fuel: usize,
    deadline: Instant,
    dependencies: BTreeSet<String>,
    search_evidence: Vec<Value>,
}
impl Engine {
    fn new(today: NaiveDate) -> Self {
        Self {
            env: BTreeMap::new(),
            today,
            fuel: 500000,
            deadline: Instant::now() + Duration::from_secs(1),
            dependencies: BTreeSet::new(),
            search_evidence: vec![],
        }
    }
    fn tick(&mut self) -> Result<(), String> {
        self.fuel = self.fuel.checked_sub(1).ok_or("程序超出计算工作量上限")?;
        if self.fuel.is_multiple_of(64) && Instant::now() > self.deadline {
            return Err("程序执行超过时间上限".into());
        }
        Ok(())
    }
    fn builtin(&self, name: &str) -> Option<Datum> {
        Some(match name {
            "today" => Datum::Date(self.today),
            "zero" => Datum::Number(Number::integer(0)),
            "one" => Datum::Number(Number::integer(1)),
            "pi" => Datum::Number(Number::Approx(std::f64::consts::PI)),
            "e" => Datum::Number(Number::Approx(std::f64::consts::E)),
            "cycle_days" => Datum::Number(Number::integer(146097)),
            "true" => Datum::Bool(true),
            "false" => Datum::Bool(false),
            "any" => Datum::Any,
            "days" | "weeks" | "months" | "years" | "strict" => Datum::Symbol(name.into()),
            _ => return None,
        })
    }
    fn evaluate(&mut self, e: &Expr) -> Result<Datum, String> {
        match self.eval(e) {
            Err(message)
                if matches!(
                    message.as_str(),
                    "除数不能为零"
                        | "负数的实数平方根不存在"
                        | "空列表没有平均值"
                        | "空列表没有极值"
                        | "日期不存在"
                ) =>
            {
                Ok(Datum::Undefined(message))
            }
            other => other,
        }
    }
    fn eval(&mut self, e: &Expr) -> Result<Datum, String> {
        self.tick()?;
        match e {
            Expr::Name(n) => {
                self.dependencies.insert(n.clone());
                self.env
                    .get(n)
                    .cloned()
                    .or_else(|| self.builtin(n))
                    .ok_or_else(|| format!("输入或此前结果 {n} 不存在；禁止前向引用"))
            }
            Expr::Number(s) => Ok(Datum::Number(Number::parse(s)?)),
            Expr::List(v) => v
                .iter()
                .map(|e| self.eval(e))
                .collect::<Result<Vec<_>, _>>()
                .map(Datum::List),
            Expr::Unary(op, e) => {
                let v = self.eval(e)?;
                match op.as_str() {
                    "-" => Ok(Datum::Number(v.number()?.neg()?)),
                    "+" => {
                        v.number()?;
                        Ok(v)
                    }
                    _ => Ok(Datum::Bool(!v.boolean()?)),
                }
            }
            Expr::Binary(op, a, b) => {
                let a = self.eval(a)?;
                if op == "&&" && !a.boolean()? {
                    return Ok(Datum::Bool(false));
                }
                if op == "||" && a.boolean()? {
                    return Ok(Datum::Bool(true));
                }
                let b = self.eval(b)?;
                match op.as_str() {
                    "&&" | "||" => Ok(Datum::Bool(b.boolean()?)),
                    "==" | "!=" | "<" | ">" | "<=" | ">=" => {
                        let c = compare(&a, &b)?;
                        Ok(Datum::Bool(match op.as_str() {
                            "==" => c.is_eq(),
                            "!=" => !c.is_eq(),
                            "<" => c.is_lt(),
                            ">" => c.is_gt(),
                            "<=" => !c.is_gt(),
                            _ => !c.is_lt(),
                        }))
                    }
                    _ => {
                        let (a, b) = (a.number()?, b.number()?);
                        Ok(Datum::Number(match op.as_str() {
                            "+" => a.add(b)?,
                            "-" => a.add(&b.neg()?)?,
                            "*" => a.mul(b)?,
                            "/" => a.div(b)?,
                            "^" => a.pow(b)?,
                            "%" => {
                                let (a, b) = (a.int()?, b.int()?);
                                Number::integer(a.checked_rem(b).ok_or("余数运算无效")?)
                            }
                            _ => return Err("未知运算符".into()),
                        }))
                    }
                }
            }
            Expr::Call(name, args) => self.call(name, args),
        }
    }
}
fn compare(a: &Datum, b: &Datum) -> Result<std::cmp::Ordering, String> {
    match (a, b) {
        (Datum::Number(a), Datum::Number(b)) => a.cmp(b),
        (Datum::Date(a), Datum::Date(b)) => Ok(a.cmp(b)),
        (Datum::Bool(a), Datum::Bool(b)) => Ok(a.cmp(b)),
        _ => Err("不能比较不同类型".into()),
    }
}
fn valid_date(d: NaiveDate) -> Result<NaiveDate, String> {
    if (1..=9999).contains(&d.year()) {
        Ok(d)
    } else {
        Err("日期超出0001—9999范围".into())
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && id
            .as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && id != "item"
}

#[async_trait]
impl Tool for Program {
    fn work_units(&self, args: &Value) -> usize {
        args["steps"].as_array().map_or(1, |s| s.len().max(1))
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Value, String> {
        let plan: Plan = serde_json::from_value(args).map_err(|_| "计算程序格式无效")?;
        if plan.inputs.len() > 32
            || plan.steps.is_empty()
            || plan.steps.len() > 16
            || plan.outputs.is_empty()
            || plan.outputs.len() > 16
        {
            return Err("程序输入、步骤或输出数量超出范围".into());
        }
        let mut engine = Engine::new(ctx.reference_time.with_timezone(&ctx.timezone).date_naive());
        let mut ids = BTreeSet::new();
        for id in plan
            .inputs
            .iter()
            .map(|i| &i.id)
            .chain(plan.steps.iter().map(|s| &s.id))
        {
            if !valid_id(id) || engine.builtin(id).is_some() || !ids.insert(id.clone()) {
                return Err(
                    "输入和步骤标识符必须唯一且不覆盖系统名称，只能以英文字母或下划线开头并包含字母数字下划线".into(),
                );
            }
        }
        let mut inputs = vec![];
        let mut records = vec![];
        for i in &plan.inputs {
            let value = source::bind(i, ctx, &mut engine)?;
            inputs.push(json!({"id":i.id,"source":i.source,"quote":i.quote,"kind":i.kind,"data":value.json()}));
            engine.env.insert(i.id.clone(), value);
        }
        for (index, s) in plan.steps.iter().enumerate() {
            tokio::task::yield_now().await;
            engine.dependencies.clear();
            let result = syntax::parse(&s.expression, false).and_then(|ast| engine.evaluate(&ast));
            match result {
                Ok(value) => {
                    records.push(json!({"id":s.id,"expression":s.expression,"dependencies":engine.dependencies,"ok":true,"data":value.json()}));
                    engine.env.insert(s.id.clone(), value);
                }
                Err(error) => {
                    records.push(
                        json!({"id":s.id,"expression":s.expression,"ok":false,"error":error}),
                    );
                    return Ok(
                        json!({"complete":false,"inputs":inputs,"steps":records,"failed_step":s.id,"skipped_steps":plan.steps[index+1..].iter().map(|s|&s.id).collect::<Vec<_>>()}),
                    );
                }
            }
        }
        let rendered = (|| -> Result<(Vec<String>, Vec<Value>), String> {
            let mut lines = vec![];
            let mut claims = vec![];
            for out in &plan.outputs {
                if !plan.steps.iter().any(|s| s.id == out.r#ref) {
                    return Err("最终答案只能引用本程序成功执行的步骤结果".into());
                }
                let value = engine.env.get(&out.r#ref).ok_or("输出引用不存在")?;
                let label = match out.label.as_str() {
                    "result" => "结果",
                    "date" => "日期",
                    "days" => "相差天数",
                    "total" => "合计",
                    "average" => "平均值",
                    "comparison" => "条件是否成立",
                    "none" => "",
                    _ => return Err("不支持的答案标签".into()),
                };
                let unit = match out.unit.as_str() {
                    "none" => "",
                    "yuan" => " 元",
                    "days" => " 天",
                    "weeks" => " 周",
                    "months" => " 个月",
                    "years" => " 年",
                    "hours" => " 小时",
                    "minutes" => " 分钟",
                    "seconds" => " 秒",
                    _ => return Err("不支持的显示单位".into()),
                };
                if !unit.is_empty() && !matches!(value, Datum::Number(_)) {
                    return Err("只有数值结果可以附加单位".into());
                }
                let text = format!(
                    "{}{}{}{}",
                    label,
                    if label.is_empty() { "" } else { "：" },
                    value.text()?,
                    unit
                );
                claims.push(json!({"ref":out.r#ref,"label":out.label,"unit":out.unit,"data":value.json(),"text":text}));
                lines.push(text);
            }
            Ok((lines, claims))
        })();
        let (mut lines, claims) = match rendered {
            Ok(result) => result,
            Err(error) => {
                return Ok(
                    json!({"complete":false,"inputs":inputs,"steps":records,"output_error":error,"skipped_steps":[]}),
                );
            }
        };
        // Preserve search limits even when a later map/reduction is the selected output.
        if !engine.search_evidence.is_empty()
            && !plan
                .outputs
                .iter()
                .any(|o| matches!(engine.env.get(&o.r#ref), Some(Datum::Search { .. })))
        {
            for e in &engine.search_evidence {
                lines.push(format!(
                    "日期搜索范围：以 {} 为参考，{}，最多检查 {} 天。{}",
                    e["anchor"].as_str().unwrap_or(""),
                    match e["direction"].as_str() {
                        Some("past") => "向过去",
                        Some("future") => "向未来",
                        _ => "前后按实际日期距离比较",
                    },
                    e["within_days"],
                    if e["range_exhausted"] == true {
                        "范围内未找到足够数量的日期。"
                    } else {
                        ""
                    }
                ));
            }
        }
        Ok(
            json!({"complete":true,"inputs":inputs,"steps":records,"claims":claims,"text":lines.join("\n"),"search_evidence":engine.search_evidence}),
        )
    }
}
