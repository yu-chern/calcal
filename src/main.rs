use calcal::{access::Access, config::Config, http::router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let access = Access::from_config(&config).await?;
    let address = format!("127.0.0.1:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!(
        "Calcal listening at http://{address} (auth: {})",
        access.mode()
    );
    axum::serve(listener, router(config, access))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
