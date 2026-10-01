pub mod config;

use crate::{
    models::{ModelAdapter, ModelRequest, ToolCall, ToolOutput},
    storage::{Store, StoreError},
    tools::{ToolContext, ToolRegistry},
};
use config::SystemConfig;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::{RwLock, Semaphore, watch};
use uuid::Uuid;

pub struct AgentService {
    pub store: Store,
    config: SystemConfig,
    model: Arc<dyn ModelAdapter>,
    pub tools: RwLock<ToolRegistry>,
    permits: Arc<Semaphore>,
    shutdown: watch::Sender<bool>,
}
#[derive(Debug)]
pub enum SubmitError {
    Store(StoreError),
    Busy,
}
impl From<StoreError> for SubmitError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
struct Stop {
    reason: &'static str,
    message: String,
}
impl From<StoreError> for Stop {
    fn from(_: StoreError) -> Self {
        Self {
            reason: "storage_error",
            message: "保存运行记录失败，已停止执行。请检查数据库。".into(),
        }
    }
}
impl AgentService {
    pub fn new(
        store: Store,
        config: SystemConfig,
        model: Arc<dyn ModelAdapter>,
        tools: ToolRegistry,
    ) -> Arc<Self> {
        let (shutdown, _) = watch::channel(false);
        Arc::new(Self {
            store,
            permits: Arc::new(Semaphore::new(config.agent.max_concurrent_runs)),
            config,
            model,
            tools: RwLock::new(tools),
            shutdown,
        })
    }
    pub async fn submit(
        self: &Arc<Self>,
        owner: &str,
        conversation: Uuid,
        id: Uuid,
        prompt: &str,
    ) -> Result<(), SubmitError> {
        match self.store.view(owner, id).await {
            Ok(existing) => {
                return if existing.conversation_id == conversation && existing.prompt == prompt {
                    Ok(())
                } else {
                    Err(StoreError::Conflict.into())
                };
            }
            Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        if *self.shutdown.borrow() {
            return Err(SubmitError::Busy);
        }
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| SubmitError::Busy)?;
        let tools = self.tools.read().await.clone();
        let reference_time = chrono::Utc::now();
        let timezone: chrono_tz::Tz = self
            .config
            .tools
            .timezone
            .parse()
            .expect("validated timezone");
        let local = reference_time.with_timezone(&timezone);
        let runtime_context = json!({"reference_time":reference_time,"today":local.date_naive(),"timezone":timezone.to_string()});
        let instructions = format!(
            "{}\n服务端提供的本轮日期上下文：{}\n实际今天可以直接使用此日期；用户的假设日期不改变实际今天。跨轮以本轮上下文为准。",
            self.config.instructions, runtime_context
        );
        let snapshot = json!({"schema_version":2,"config":self.config,"instructions":instructions,
            "tools":tools.definitions(),"reference_time":reference_time,"runtime_context":runtime_context});
        if !self
            .store
            .begin(
                owner,
                conversation,
                id,
                prompt,
                snapshot,
                self.config.duration(),
            )
            .await?
        {
            return Ok(());
        }
        crate::prompt::print_prompt(prompt);
        let service = self.clone();
        let mut shutdown = self.shutdown.subscribe();
        tokio::spawn(async move {
            let _permit = permit;
            let context = ToolContext {
                reference_time,
                timezone,
            };
            let result = tokio::select! {
                result = tokio::time::timeout(service.config.duration(), service.run(id,conversation,tools,context,instructions)) => {
                    match result { Ok(value) => value, Err(_) => Err(Stop {reason:"timeout",message:"运行超时，请缩小问题范围后重试。".into()}) }
                }
                _ = shutdown.wait_for(|stopping| *stopping) => Err(Stop {reason:"cancelled",message:"服务正在停止，本次运行已取消。".into()}),
            };
            if let Err(stop) = result
                && service
                    .store
                    .finish(id, stop.reason, None, Some(&stop.message))
                    .await
                    .is_err()
            {
                eprintln!(
                    "Agent could not persist terminal status; database recovery is required."
                );
            }
        });
        Ok(())
    }
    async fn run(
        &self,
        id: Uuid,
        conversation: Uuid,
        tools: ToolRegistry,
        context: ToolContext,
        instructions: String,
    ) -> Result<(), Stop> {
        let history = self
            .store
            .history(
                conversation,
                id,
                self.config.agent.history_turns,
                self.config.agent.max_context_chars,
            )
            .await?;
        let mut request = ModelRequest {
            instructions,
            messages: history,
            tools: tools.definitions(),
            turns: Vec::new(),
            outputs: Vec::new(),
        };
        let mut calls = 0;
        for step in 1..=self.config.agent.max_steps {
            let size = request
                .messages
                .iter()
                .map(|m| m.text.chars().count())
                .sum::<usize>()
                + serde_json::to_string(&request.turns)
                    .unwrap_or_default()
                    .chars()
                    .count()
                + serde_json::to_string(&request.outputs)
                    .unwrap_or_default()
                    .chars()
                    .count()
                + request.instructions.chars().count()
                + serde_json::to_string(&request.tools)
                    .unwrap_or_default()
                    .chars()
                    .count();
            if size > self.config.agent.max_context_chars {
                return Err(Stop {
                    reason: "context_limit",
                    message: "本次问题的上下文过长，请新建对话或缩小问题范围。".into(),
                });
            }
            self.store
                .activity(
                    id,
                    if step == 1 {
                        "正在分析问题"
                    } else {
                        "正在分析工具结果"
                    },
                    step,
                )
                .await?;
            let turn = self
                .model
                .generate(&request)
                .await
                .map_err(|message| Stop {
                    reason: "model_error",
                    message,
                })?;
            self.store
                .append(
                    id,
                    "model_turn",
                    json!({"schema_version":1,"step":step,"turn":turn}),
                )
                .await?;
            if turn.tool_calls.is_empty() || turn.refused {
                if turn.text.trim().is_empty() {
                    return Err(Stop {
                        reason: "model_error",
                        message: "模型未返回有效回答。".into(),
                    });
                }
                self.store
                    .finish(
                        id,
                        if turn.refused { "refused" } else { "completed" },
                        Some(&turn.text),
                        None,
                    )
                    .await?;
                return Ok(());
            }
            // A clarification is terminal and takes precedence over every computation,
            // even if the model accidentally requested both in the same response.
            if let Some(question) = turn
                .tool_calls
                .iter()
                .find(|c| tools.is_clarification(&c.name))
            {
                if calls >= self.config.agent.max_tool_calls {
                    return Err(Stop {
                        reason: "budget_exhausted",
                        message: "已达到工具调用上限，请重新发送。".into(),
                    });
                }
                calls += 1;
                let result = self
                    .execute_tool(id, step, question, &tools, &context)
                    .await?;
                for other in turn.tool_calls.iter().filter(|c| c.id != question.id) {
                    self.store.append(id,"tool_skipped",json!({"schema_version":1,"step":step,"call":other,"reason":"clarification_pending"})).await?;
                }
                if result["ok"] == true {
                    let text = result["data"]["text"].as_str().ok_or_else(|| Stop {
                        reason: "model_error",
                        message: "澄清问题格式无效".into(),
                    })?;
                    self.store.append(id,"clarification",json!({"schema_version":1,"step":step,"question":result["data"],"messages":request.messages})).await?;
                    self.store
                        .finish(id, "clarification", Some(text), None)
                        .await?;
                    return Ok(());
                }
                // Invalid clarification arguments may be repaired, but do not execute
                // other work before resolving the question. Reply to every call ID.
                let outputs = turn.tool_calls.iter().map(|c| ToolOutput {
                    call_id:c.id.clone(),
                    result:if c.id==question.id {result.clone()} else {json!({"ok":false,"error":{"code":"CLARIFICATION_PENDING","message":"等待有效澄清问题，此工具未执行"}})},
                }).collect();
                request.turns.push(turn);
                request.outputs.push(outputs);
                continue;
            }
            let work = turn.tool_calls.iter().fold(0usize, |total, c| {
                total.saturating_add(tools.work_units(&c.name, &c.arguments))
            });
            if step == self.config.agent.max_steps
                || calls.saturating_add(work) > self.config.agent.max_tool_calls
            {
                return Err(Stop {
                    reason: "budget_exhausted",
                    message: "已达到本次运行的步骤或工具调用上限，请拆分问题后重试。".into(),
                });
            }
            let mut outputs = Vec::new();
            for call in &turn.tool_calls {
                calls += tools.work_units(&call.name, &call.arguments);
                let result = self.execute_tool(id, step, call, &tools, &context).await?;
                outputs.push(ToolOutput {
                    call_id: call.id.clone(),
                    result,
                });
            }
            request.turns.push(turn);
            request.outputs.push(outputs);
        }
        Err(Stop {
            reason: "budget_exhausted",
            message: "已达到运行上限。".into(),
        })
    }
    async fn execute_tool(
        &self,
        id: Uuid,
        step: usize,
        call: &ToolCall,
        tools: &ToolRegistry,
        context: &ToolContext,
    ) -> Result<Value, Stop> {
        let definition = tools.definition(&call.name);
        let label = definition
            .map(|d| d.activity.as_str())
            .unwrap_or("正在检查工具参数");
        self.store.append(id,"tool_call",json!({"schema_version":2,"step":step,"call":call,"version":definition.map(|d|&d.version),"work_units":tools.work_units(&call.name,&call.arguments)})).await?;
        self.store.activity(id, label, step).await?;
        let result = tools
            .execute(
                &call.name,
                call.arguments.clone(),
                context,
                Duration::from_secs(self.config.agent.tool_timeout_seconds),
            )
            .await;
        self.store
            .append(
                id,
                "tool_result",
                json!({"schema_version":2,"step":step,"call_id":call.id,"result":result}),
            )
            .await?;
        self.store
            .activity(
                id,
                if result["ok"] == true {
                    "工具执行完成"
                } else {
                    "工具返回错误，正在调整"
                },
                step,
            )
            .await?;
        Ok(result)
    }
    pub async fn shutdown(&self) {
        self.shutdown.send_replace(true);
        let _ = self
            .permits
            .acquire_many(self.config.agent.max_concurrent_runs as u32)
            .await;
    }
}
