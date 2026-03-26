use std::path::PathBuf;
use tracing::{info, warn, error};

mod comms;
mod config;
mod connection;
mod health;
mod tunnel;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("nucleus_agent=info".parse().unwrap()),
        )
        .init();

    info!("Nucleus Agent starting...");

    // Resolve config path: CLI --config flag > AGENT_CONFIG env > default
    let config_path = resolve_config_path();

    let mut config = match config::AgentConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config from {:?}: {}", config_path, e);
            std::process::exit(1);
        }
    };

    // Allow env var overrides for Docker deployments
    // AGENT_SERVER_URL overrides [server].url
    // AGENT_TOKEN overrides [server].token
    if let Ok(url) = std::env::var("AGENT_SERVER_URL") {
        warn!("Overriding server URL from AGENT_SERVER_URL env");
        config.server.url = url;
    }
    if let Ok(token) = std::env::var("AGENT_TOKEN") {
        warn!("Overriding token from AGENT_TOKEN env");
        config.server.token = token;
    }

    if config.server.token.is_empty() || config.server.token == "your-device-auth-token" {
        error!("Token is not configured! Set [server].token in config or AGENT_TOKEN env var.");
        error!("The token must be the device UUID from the Nucleus Portal.");
        std::process::exit(1);
    }

    info!("Config loaded. Server: {}", config.server.url);

    // Run the main connection loop
    connection::run(config).await;
}

/// Resolve config file path from CLI args, env var, or default.
/// Supports: `nucleus-agent --config /path/to/agent.toml`
fn resolve_config_path() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();

    // Check for --config CLI flag
    for i in 0..args.len() {
        if args[i] == "--config" {
            if let Some(path) = args.get(i + 1) {
                info!("Using config from --config flag: {}", path);
                return PathBuf::from(path);
            }
        }
    }

    // Check AGENT_CONFIG env var
    if let Ok(path) = std::env::var("AGENT_CONFIG") {
        info!("Using config from AGENT_CONFIG env: {}", path);
        return PathBuf::from(path);
    }

    // Default path
    let default = PathBuf::from("/etc/nucleus/agent.toml");
    info!("Using default config path: {:?}", default);
    default
}
