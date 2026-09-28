pub mod config;

use crate::{
    models::{ModelAdapter, ModelRequest, ToolOutput},
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
        let snapshot = json!({"schema_version":1,"config":self.config,"instructions":self.config.instructions,
            "tools":tools.definitions(),"reference_time":reference_time});
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
                timezone: service
                    .config
                    .tools
                    .timezone
                    .parse()
                    .expect("validated timezone"),
            };
            let result = tokio::select! {
                result = tokio::time::timeout(service.config.duration(), service.run(id,conversation,tools,context)) => {
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
            instructions: self.config.instructions.clone(),
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
            if step == self.config.agent.max_steps
                || calls + turn.tool_calls.len() > self.config.agent.max_tool_calls
            {
                return Err(Stop {
                    reason: "budget_exhausted",
                    message: "已达到本次运行的步骤或工具调用上限，请拆分问题后重试。".into(),
                });
            }
            let mut outputs = Vec::new();
            for call in &turn.tool_calls {
                calls += 1;
                let definition = tools.definition(&call.name);
                let label = definition
                    .map(|d| d.activity.as_str())
                    .unwrap_or("正在检查工具参数");
                self.store.append(id,"tool_call",json!({"schema_version":1,"call":call,"version":definition.map(|d|&d.version)})).await?;
                self.store.activity(id, label, step).await?;
                let result: Value = tools
                    .execute(
                        &call.name,
                        call.arguments.clone(),
                        &context,
                        Duration::from_secs(self.config.agent.tool_timeout_seconds),
                    )
                    .await;
                self.store
                    .append(
                        id,
                        "tool_result",
                        json!({"schema_version":1,"call_id":call.id,"result":result}),
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
    pub async fn shutdown(&self) {
        self.shutdown.send_replace(true);
        let _ = self
            .permits
            .acquire_many(self.config.agent.max_concurrent_runs as u32)
            .await;
    }
}
