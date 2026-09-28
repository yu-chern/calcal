use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

pub const DEFAULT_REASONING_EFFORT: &str = "medium";

fn default_reasoning_effort() -> String {
    DEFAULT_REASONING_EFFORT.into()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    pub model: ModelConfig,
    pub agent: AgentConfig,
    pub database: DatabaseConfig,
    pub tools: ToolsConfig,
    #[serde(skip)]
    pub instructions: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    pub base_url: String,
    pub api_key_env: String,
    pub request_timeout_seconds: u64,
    pub max_output_tokens: u32,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub system_prompt_file: String,
    pub max_steps: usize,
    pub max_tool_calls: usize,
    pub run_timeout_seconds: u64,
    pub tool_timeout_seconds: u64,
    pub max_concurrent_runs: usize,
    pub history_turns: usize,
    pub max_context_chars: usize,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub url_env: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    pub timezone: String,
    pub definitions: Vec<String>,
}

impl SystemConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let source =
            std::fs::read_to_string(path).map_err(|_| "无法读取 system_config/config.toml")?;
        let mut config: Self =
            toml::from_str(&source).map_err(|_| "系统配置 TOML 无效或含未知字段")?;
        config.validate()?;
        let root = path.parent().unwrap_or(Path::new("."));
        config.instructions = std::fs::read_to_string(root.join(&config.agent.system_prompt_file))
            .map_err(|_| "无法读取系统提示词")?;
        if config.instructions.trim().is_empty() || config.instructions.chars().count() > 16000 {
            return Err("系统提示词为空或过长".into());
        }
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(
            self.model.reasoning_effort.as_str(),
            "none" | "low" | "medium" | "high" | "xhigh" | "max"
        ) {
            return Err("reasoning_effort 必须为 none、low、medium、high、xhigh 或 max".into());
        }
        let a = &self.agent;
        if !(1..=32).contains(&a.max_steps)
            || !(1..=64).contains(&a.max_tool_calls)
            || !(1..=600).contains(&a.run_timeout_seconds)
            || !(1..=30).contains(&a.tool_timeout_seconds)
            || !(1..=16).contains(&a.max_concurrent_runs)
            || !(1..=100).contains(&a.history_turns)
            || !(4000..=200000).contains(&a.max_context_chars)
            || !(1..=300).contains(&self.model.request_timeout_seconds)
            || !(256..=32768).contains(&self.model.max_output_tokens)
            || self.model.model.trim().is_empty()
            || self.tools.definitions.len() > 64
        {
            return Err("系统配置超出允许范围".into());
        }
        self.tools
            .timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| "无效时区")?;
        let url = reqwest::Url::parse(&self.model.base_url).map_err(|_| "无效模型地址")?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))))
        {
            return Err("模型地址必须为 HTTPS，或本地回环 HTTP；不能包含凭据或查询参数".into());
        }
        Ok(())
    }
    pub fn duration(&self) -> Duration {
        Duration::from_secs(self.agent.run_timeout_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_effort_defaults_and_rejects_invalid_values() {
        let source = include_str!("../../system_config/config.toml");
        let omitted = source.replace("reasoning_effort = \"medium\"\n", "");
        let mut config: SystemConfig = toml::from_str(&omitted).unwrap();
        assert_eq!(config.model.reasoning_effort, DEFAULT_REASONING_EFFORT);
        assert!(config.validate().is_ok());
        config.model.reasoning_effort = "invalid".into();
        assert!(config.validate().unwrap_err().contains("reasoning_effort"));
    }
}
