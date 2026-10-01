use super::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

pub struct Clarification;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    question: String,
    reason: String,
    options: Vec<String>,
}
#[async_trait]
impl Tool for Clarification {
    async fn execute(&self, args: Value, _: &ToolContext) -> Result<Value, String> {
        let q: Question = serde_json::from_value(args).map_err(|_| "澄清参数无效")?;
        if q.question.trim().is_empty()
            || q.question.chars().count() > 1000
            || !matches!(
                q.reason.as_str(),
                "ambiguous" | "conflicting" | "missing_information" | "unsupported"
            )
            || q.options.len() > 4
            || q.options
                .iter()
                .any(|s| s.trim().is_empty() || s.chars().count() > 200)
        {
            return Err("澄清问题、原因或选项无效".into());
        }
        let mut text = q.question.clone();
        for (i, option) in q.options.iter().enumerate() {
            text.push_str(&format!("\n{}. {}", i + 1, option));
        }
        Ok(json!({"question":q.question,"reason":q.reason,"options":q.options,"text":text}))
    }
}
