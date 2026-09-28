pub mod openai;
use crate::tools::ToolDefinition;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub call_id: String,
    pub result: Value,
}
#[derive(Clone)]
pub struct ModelRequest {
    pub instructions: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub turns: Vec<ModelTurn>,
    pub outputs: Vec<Vec<ToolOutput>>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ModelTurn {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub refused: bool,
    pub usage: Value,
    /// Provider-specific continuation, never shown in the user interface.
    pub continuation: Value,
}
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn generate(&self, request: &ModelRequest) -> Result<ModelTurn, String>;
}
