use super::{Tool, ToolContext};
use async_trait::async_trait;
use serde_json::{Value, json};
pub struct Calculator;
#[async_trait]
impl Tool for Calculator {
    async fn execute(&self, arguments: Value, _: &ToolContext) -> Result<Value, String> {
        let expression = arguments["expression"].as_str().ok_or("缺少表达式")?;
        if expression.len() > 512 {
            return Err("表达式过长".into());
        }
        // Pure, bounded expression language: no code execution, files, or network.
        let value = meval::eval_str(expression).map_err(|_| "表达式无效或包含不支持的函数")?;
        if !value.is_finite() {
            return Err("计算结果不是有限数值，请检查除零、溢出或函数定义域".into());
        }
        Ok(json!({"value":value,"approximate":true,"precision":"IEEE-754 binary64"}))
    }
}
