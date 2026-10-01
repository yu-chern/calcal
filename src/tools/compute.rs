use super::{Tool, ToolContext, calculator::calculate, calendar::Calendar, date_search::Search};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

pub struct Compute;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    steps: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    id: String,
    action: Value,
}
#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Action {
    #[serde(rename = "calculate")]
    Calculate {
        expression: String,
        variables: Vec<Variable>,
    },
    #[serde(rename = "calendar")]
    Calendar {
        operation: String,
        date: Option<String>,
        start: Option<String>,
        end: Option<String>,
        amount: Option<f64>,
        unit: Option<String>,
    },
    #[serde(rename = "find_dates")]
    FindDates { query: Search },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variable {
    name: String,
    value: f64,
}

// References replace an entire JSON value. No interpolation, code, I/O or forward references.
fn resolve(value: &Value, results: &Value, depth: usize) -> Result<Value, String> {
    if depth > 12 {
        return Err("参数嵌套过深".into());
    }
    match value {
        Value::Object(object) if object.contains_key("ref") => {
            let path = object["ref"]
                .as_str()
                .ok_or("引用必须是 JSON pointer 字符串")?;
            if object.len() != 1 || path.len() > 128 || !path.starts_with('/') {
                return Err("引用格式无效".into());
            }
            let v = results
                .pointer(path)
                .ok_or("引用未完成、失败或不存在的步骤字段")?;
            if !v.is_number() && !v.is_string() {
                return Err("只能引用数值或字符串字段".into());
            }
            Ok(v.clone())
        }
        Value::Object(object) => object
            .iter()
            .map(|(k, v)| Ok((k.clone(), resolve(v, results, depth + 1)?)))
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        Value::Array(array) => array
            .iter()
            .map(|v| resolve(v, results, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        _ => Ok(value.clone()),
    }
}
async fn execute_action(action: Value, context: &ToolContext) -> Result<Value, String> {
    match serde_json::from_value::<Action>(action).map_err(|_| "步骤参数类型或字段无效")?
    {
        Action::Calculate {
            expression,
            variables,
        } => calculate(
            &expression,
            &variables
                .into_iter()
                .map(|v| (v.name, v.value))
                .collect::<Vec<_>>(),
        ),
        Action::Calendar {
            operation,
            date,
            start,
            end,
            amount,
            unit,
        } => {
            if amount.is_some_and(|n| !n.is_finite() || n.fract() != 0.0 || n.abs() > 100000.0) {
                return Err("日期偏移必须是范围内的整数；不自动舍入中间结果".into());
            }
            Calendar.execute(json!({"operation":operation,"date":date,"start":start,"end":end,"amount":amount.map(|n|n as i64),"unit":unit}),context).await
        }
        Action::FindDates { query } => query.execute().await,
    }
}
#[async_trait]
impl Tool for Compute {
    fn work_units(&self, args: &Value) -> usize {
        args["steps"]
            .as_array()
            .map_or(1, |steps| steps.len().max(1))
    }
    async fn execute(&self, args: Value, context: &ToolContext) -> Result<Value, String> {
        let plan: Plan = serde_json::from_value(args).map_err(|_| "计算计划无效")?;
        if plan.steps.is_empty() || plan.steps.len() > 16 {
            return Err("计划必须有1至16个步骤".into());
        }
        let mut ids = HashSet::new();
        for step in &plan.steps {
            if step.id.is_empty()
                || step.id.len() > 32
                || !step
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !ids.insert(&step.id)
            {
                return Err("步骤ID无效或重复".into());
            }
        }
        let mut results = json!({});
        let mut records = Vec::new();
        for step in &plan.steps {
            tokio::task::yield_now().await;
            let resolved = resolve(&step.action, &results, 0);
            let result = match &resolved {
                Ok(action) => execute_action(action.clone(), context).await,
                Err(error) => Err(error.clone()),
            };
            match result {
                Ok(value) => {
                    results[&step.id] = value.clone();
                    records.push(json!({"id":step.id,"action":step.action,"resolved_action":resolved.ok(),"ok":true,"data":value}));
                }
                Err(message) => {
                    records.push(json!({"id":step.id,"action":step.action,"resolved_action":resolved.ok(),"ok":false,"error":message}));
                    return Ok(
                        json!({"complete":false,"steps":records,"failed_step":step.id,
                        "skipped_steps":plan.steps.iter().skip(records.len()).map(|s| &s.id).collect::<Vec<_>>() }),
                    );
                }
            }
        }
        Ok(json!({"complete":true,"steps":records}))
    }
}
