mod calculator;
mod calendar;
mod clarification;
mod clarification_v2;
pub use clarification_v2::selection as clarification_selection;
mod compute;
mod date_search;
mod program;
mod reply;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub name: String,
    pub version: String,
    pub binding: String,
    pub description: String,
    pub activity: String,
    pub input_schema: Value,
}
#[derive(Clone)]
pub struct ToolContext {
    pub reference_time: DateTime<Utc>,
    pub timezone: chrono_tz::Tz,
    /// User messages only, numbered in the model instructions.
    pub sources: Vec<String>,
    pub selections: BTreeMap<usize, String>,
    /// Successful computed answers from this conversation only.
    pub verified_answers: BTreeMap<String, Value>,
}
#[async_trait]
pub trait Tool: Send + Sync {
    fn work_units(&self, _: &Value) -> usize {
        1
    }
    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<Value, String>;
}
struct RegisteredTool {
    definition: ToolDefinition,
    validator: jsonschema::Validator,
    implementation: Arc<dyn Tool>,
}
/// Cloning the registry takes a cheap immutable snapshot for an entire run.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    active: BTreeMap<String, Arc<RegisteredTool>>,
}
impl ToolRegistry {
    pub fn register(
        &mut self,
        definition: ToolDefinition,
        implementation: Arc<dyn Tool>,
    ) -> Result<(), String> {
        if definition.name.is_empty()
            || definition.name.len() > 64
            || !definition
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || definition.version.is_empty()
            || definition.description.is_empty()
            || definition.activity.is_empty()
            || definition.activity.chars().count() > 80
        {
            return Err("无效工具定义".into());
        }
        let validator = jsonschema::validator_for(&definition.input_schema)
            .map_err(|_| "无效工具参数 schema")?;
        // Existing snapshots retain their Arc, including the exact implementation.
        self.active.insert(
            definition.name.clone(),
            Arc::new(RegisteredTool {
                definition,
                validator,
                implementation,
            }),
        );
        Ok(())
    }
    pub fn disable(&mut self, name: &str) {
        self.active.remove(name);
    }
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.active.values().map(|t| t.definition.clone()).collect()
    }
    pub fn definition(&self, name: &str) -> Option<&ToolDefinition> {
        self.active.get(name).map(|t| &t.definition)
    }
    pub fn work_units(&self, name: &str, args: &Value) -> usize {
        self.active
            .get(name)
            .map_or(1, |t| t.implementation.work_units(args))
    }
    pub fn is_clarification(&self, name: &str) -> bool {
        self.definition(name)
            .is_some_and(|d| matches!(d.binding.as_str(), "clarification.v1" | "clarification.v2"))
    }
    pub fn load(root: &Path, paths: &[String]) -> Result<Self, String> {
        let implementations: BTreeMap<&str, Arc<dyn Tool>> = BTreeMap::from([
            (
                "calculator.v1",
                Arc::new(calculator::Calculator) as Arc<dyn Tool>,
            ),
            ("calendar.v1", Arc::new(calendar::Calendar) as Arc<dyn Tool>),
            ("compute.v1", Arc::new(compute::Compute) as Arc<dyn Tool>),
            ("compute.v2", Arc::new(program::Program) as Arc<dyn Tool>),
            ("respond.v1", Arc::new(reply::Reply) as Arc<dyn Tool>),
            (
                "clarification.v2",
                Arc::new(clarification_v2::Clarification) as Arc<dyn Tool>,
            ),
            (
                "clarification.v1",
                Arc::new(clarification::Clarification) as Arc<dyn Tool>,
            ),
        ]);
        let mut registry = Self::default();
        for path in paths {
            let definition: ToolDefinition = serde_json::from_str(
                &std::fs::read_to_string(root.join(path)).map_err(|_| "无法读取工具定义")?,
            )
            .map_err(|_| "工具定义 JSON 无效")?;
            if registry.active.contains_key(&definition.name) {
                return Err("工具名称重复".into());
            }
            let implementation = implementations
                .get(definition.binding.as_str())
                .ok_or("工具绑定的实现不存在")?
                .clone();
            registry.register(definition, implementation)?;
        }
        if registry
            .definitions()
            .iter()
            .any(|d| d.binding == "compute.v2")
            && registry.definitions().iter().any(|d| {
                !matches!(
                    d.binding.as_str(),
                    "compute.v2" | "clarification.v2" | "respond.v1"
                )
            })
        {
            return Err(
                "来源验证模式只能注册compute.v2、clarification.v2与respond.v1，禁止旧工具绕过验证"
                    .into(),
            );
        }
        Ok(registry)
    }
    pub async fn execute(
        &self,
        name: &str,
        args: Value,
        context: &ToolContext,
        timeout: Duration,
    ) -> Value {
        let Some(tool) = self.active.get(name) else {
            return failure("UNKNOWN_TOOL", "工具不存在或已停用");
        };
        if !tool.validator.is_valid(&args) {
            return failure(
                "INVALID_ARGUMENTS",
                "参数不符合工具定义，请检查字段、类型与范围",
            );
        }
        match tokio::time::timeout(timeout, tool.implementation.execute(args, context)).await {
            Ok(Ok(data))
                if data.to_string().len() <= 16000
                    && matches!(
                        tool.definition.binding.as_str(),
                        "compute.v1" | "compute.v2" | "respond.v1"
                    )
                    && data["complete"] == false =>
            {
                json!({"ok":false,"error":{"code":"PLAN_FAILED","message":"组合计算未完成；失败后的步骤未执行，请检查条件或澄清后继续"},"data":data})
            }
            Ok(Ok(data)) if data.to_string().len() <= 16000 => json!({"ok":true,"data":data}),
            Ok(Ok(_)) => failure("RESULT_TOO_LARGE", "工具结果过大"),
            Ok(Err(message)) => failure("TOOL_ERROR", &message),
            Err(_) => failure("TOOL_TIMEOUT", "工具执行超时"),
        }
    }
}
fn failure(code: &str, message: &str) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registry() -> ToolRegistry {
        ToolRegistry::load(
            Path::new("system_config"),
            &[
                "tools/calculator.v1.json".into(),
                "tools/calendar.v1.json".into(),
            ],
        )
        .unwrap()
    }
    #[tokio::test]
    async fn rejects_bad_arguments_and_retains_disabled_snapshot() {
        let mut registry = registry();
        let snapshot = registry.clone();
        registry.disable("calculator");
        let ctx = ToolContext {
            reference_time: Utc::now(),
            timezone: chrono_tz::Europe::Berlin,
            sources: vec![],
            selections: BTreeMap::new(),
            verified_answers: BTreeMap::new(),
        };
        let args = json!({"expression":"(12 + 3) * 4"});
        assert_eq!(
            snapshot
                .execute("calculator", args.clone(), &ctx, Duration::from_secs(1))
                .await["data"]["value"],
            60.0
        );
        assert_eq!(
            registry
                .execute("calculator", args, &ctx, Duration::from_secs(1))
                .await["error"]["code"],
            "UNKNOWN_TOOL"
        );
        for args in [
            json!({"expression":3}),
            json!({"expression":"2+2","command":"bad"}),
            json!({"expression":"1/0"}),
            json!({"expression":"sqrt(-1)"}),
        ] {
            assert_eq!(
                snapshot
                    .execute("calculator", args, &ctx, Duration::from_secs(1))
                    .await["ok"],
                false
            );
        }
    }
}

#[cfg(test)]
mod compute_tests;
