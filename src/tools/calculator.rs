use super::{Tool, ToolContext};
use async_trait::async_trait;
use serde_json::{Value, json};
pub struct Calculator;
pub(super) fn calculate(expression: &str, variables: &[(String, f64)]) -> Result<Value, String> {
    if expression.is_empty() || expression.len() > 512 || variables.len() > 16 {
        return Err("表达式或变量数量超出范围".into());
    }
    let mut context = meval::Context::new();
    let mut names = std::collections::HashSet::new();
    for (name, value) in variables {
        if name.is_empty()
            || name.len() > 32
            || !name.bytes().all(|c| c.is_ascii_alphabetic() || c == b'_')
            || matches!(name.as_str(), "pi" | "e")
            || !names.insert(name)
            || !value.is_finite()
        {
            return Err("变量名无效、重复、覆盖常数或变量不是有限数值".into());
        }
        context.var(name, *value);
    }
    // Pure bounded expressions, with typed variables rather than string substitution.
    let expr: meval::Expr = expression.parse().map_err(|_| "表达式无效")?;
    let value = expr
        .eval_with_context(context)
        .map_err(|_| "表达式包含未知变量或不支持的函数")?;
    if !value.is_finite() {
        return Err("计算结果不是有限数值，请检查除零、溢出或函数定义域".into());
    }
    Ok(json!({"value":value,"approximate":true,"precision":"IEEE-754 binary64"}))
}
#[async_trait]
impl Tool for Calculator {
    async fn execute(&self, arguments: Value, _: &ToolContext) -> Result<Value, String> {
        let expression = arguments["expression"].as_str().ok_or("缺少表达式")?;
        calculate(expression, &[])
    }
}
