use calcal::{access::Access, config::Config, http::router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Existing exported variables take precedence over the trusted local .env.
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path("deploy/runtime/postgres.env");
    let config = Config::from_env()?;
    let system_path =
        std::env::var("SYSTEM_CONFIG").unwrap_or_else(|_| "system_config/config.toml".into());
    let path = std::path::Path::new(&system_path);
    let system = calcal::agent::config::SystemConfig::load(path)?;
    if system.model.provider != "openai" {
        return Err("未注册该模型 provider；请添加 ModelAdapter".into());
    }
    let key = std::env::var(&system.model.api_key_env)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or("缺少模型 API Key 环境变量")?;
    let database_url = std::env::var(&system.database.url_env)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or("缺少 DATABASE_URL，请配置 Postgres")?;
    let model = std::sync::Arc::new(calcal::models::openai::OpenAi::new(
        system.model.clone(),
        key,
    )?);
    let tools = calcal::tools::ToolRegistry::load(
        path.parent().unwrap_or(std::path::Path::new(".")),
        &system.tools.definitions,
    )?;
    let store = calcal::storage::Store::connect(&database_url).await?;
    let agent = calcal::agent::AgentService::new(store, system, model, tools);
    let access = Access::from_config(&config).await?;
    let address = format!("127.0.0.1:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!(
        "Calcal listening at http://{address} (auth: {})",
        access.mode()
    );
    axum::serve(listener, router(config, access, Some(agent.clone())))
        .with_graceful_shutdown(async move {
            #[cfg(unix)]
            {
                let mut terminate =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("signal handler");
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
            agent.shutdown().await;
        })
        .await?;
    Ok(())
}
