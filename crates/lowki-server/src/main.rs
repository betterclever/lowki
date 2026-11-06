use lowki_server::{build_state, config::Config, serve, spawn_janitor};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if std::env::var("LOWKI_LOG_JSON").is_ok_and(|v| v == "true" || v == "1") {
        fmt().with_env_filter(filter).json().init();
    } else {
        fmt().with_env_filter(filter).init();
    }

    let config = Config::from_env()?;
    let bind = config.bind;
    tracing::info!(
        public_url = %config.public_url,
        rp_id = %config.rp_id,
        oidc = config.oidc.as_ref().map(|o| o.provider_id.as_str()).unwrap_or("off"),
        dev_mode = config.dev_mode,
        "starting lowki-server"
    );
    let state = build_state(config, None).await?;
    spawn_janitor(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "listening");
    serve(state, listener).await
}
