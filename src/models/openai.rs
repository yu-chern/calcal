use super::*;
use crate::agent::config::ModelConfig;
use serde_json::json;
use std::{collections::HashSet, time::Duration};

pub struct OpenAi {
    config: ModelConfig,
    client: reqwest::Client,
    api_key: String,
}
impl OpenAi {
    pub fn new(config: ModelConfig, api_key: String) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "无法创建模型客户端")?;
        Ok(Self {
            config,
            client,
            api_key,
        })
    }
    fn body(&self, request: &ModelRequest) -> Value {
        let mut input: Vec<Value> = request
            .messages
            .iter()
            .map(|m| json!({"role":m.role,"content":m.text}))
            .collect();
        for (i, turn) in request.turns.iter().enumerate() {
            if let Some(items) = turn.continuation.as_array() {
                input.extend(items.clone());
            }
            if let Some(outputs) = request.outputs.get(i) {
                input.extend(outputs.iter().map(|o| json!({"type":"function_call_output","call_id":o.call_id,"output":o.result.to_string()})));
            }
        }
        let tools: Vec<Value> = request.tools.iter().map(|t| json!({"type":"function","name":t.name,"description":t.description,"parameters":t.input_schema,"strict":true})).collect();
        json!({"model":self.config.model,"instructions":request.instructions,"input":input,"tools":tools,
            "reasoning":{"effort":self.config.reasoning_effort},
            "store":false,"include":["reasoning.encrypted_content"],"parallel_tool_calls":false,
            "max_output_tokens":self.config.max_output_tokens})
    }
}
#[async_trait]
impl ModelAdapter for OpenAi {
    async fn generate(&self, request: &ModelRequest) -> Result<ModelTurn, String> {
        let mut response = self
            .client
            .post(format!(
                "{}/responses",
                self.config.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.api_key)
            .json(&self.body(request))
            .send()
            .await
            .map_err(|_| "模型连接失败或超时")?;
        if !response.status().is_success() {
            return Err(format!(
                "模型服务请求失败（HTTP {}），请检查模型配置、额度或稍后重试",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "模型响应读取失败")? {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err("模型响应过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes).map_err(|_| "模型响应不是有效 JSON")?;
        parse_response(body)
    }
}
fn parse_response(body: Value) -> Result<ModelTurn, String> {
    if body["status"] != "completed" {
        return Err("模型响应未完成，请重试或调整输出额度".into());
    }
    let items = body["output"].as_array().ok_or("模型响应缺少 output")?;
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut ids = HashSet::new();
    let mut refused = false;
    for item in items {
        match item["type"].as_str() {
            Some("function_call") => {
                let id = item["call_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or("工具调用缺少 ID")?;
                if !ids.insert(id) {
                    return Err("模型返回重复工具调用 ID".into());
                }
                let name = item["name"].as_str().ok_or("工具调用缺少名称")?;
                let raw = item["arguments"].as_str().ok_or("工具调用缺少参数")?;
                // Malformed arguments become a normal validation error from the executor.
                let arguments = serde_json::from_str(raw).unwrap_or(Value::Null);
                calls.push(ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments,
                });
            }
            Some("message") => {
                for content in item["content"].as_array().ok_or("模型消息内容无效")? {
                    match content["type"].as_str() {
                        Some("output_text") => {
                            text.push_str(content["text"].as_str().ok_or("模型文本无效")?)
                        }
                        Some("refusal") => {
                            refused = true;
                            text.push_str(
                                content["refusal"]
                                    .as_str()
                                    .unwrap_or("模型无法完成此请求。"),
                            );
                        }
                        _ => return Err("模型返回不支持的消息类型".into()),
                    }
                }
            }
            Some("reasoning") => {}
            _ => return Err("模型返回不支持的输出类型".into()),
        }
    }
    if calls.is_empty() && text.trim().is_empty() {
        return Err("模型未返回回答或工具调用".into());
    }
    Ok(ModelTurn {
        text,
        tool_calls: calls,
        refused,
        usage: body["usage"].clone(),
        continuation: body["output"].clone(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sends_luna_and_configured_reasoning_effort_in_responses_format() {
        let config = crate::agent::config::SystemConfig::load(std::path::Path::new(
            "system_config/config.toml",
        ))
        .unwrap();
        let mut adapter = OpenAi::new(config.model, "test-only".into()).unwrap();
        let request = ModelRequest {
            instructions: String::new(),
            messages: vec![],
            tools: vec![],
            turns: vec![],
            outputs: vec![],
        };
        let body = adapter.body(&request);
        assert_eq!(body["model"], "gpt-6-luna");
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert!(body.get("reasoning_effort").is_none());
        adapter.config.reasoning_effort = "high".into();
        assert_eq!(adapter.body(&request)["reasoning"]["effort"], "high");
    }

    #[test]
    fn preserves_reasoning_and_detects_incomplete_or_duplicate_calls() {
        let body = json!({"status":"completed","output":[{"type":"reasoning","encrypted_content":"opaque"},{"type":"function_call","call_id":"c1","name":"calculator","arguments":"{}"}]});
        let turn = parse_response(body.clone()).unwrap();
        assert_eq!(turn.continuation[0]["encrypted_content"], "opaque");
        assert!(parse_response(json!({"status":"incomplete","output":[]})).is_err());
        let mut duplicate = body;
        let call = duplicate["output"][1].clone();
        duplicate["output"].as_array_mut().unwrap().push(call);
        assert!(parse_response(duplicate).is_err());
    }
}
