mod claude;
mod docs_rs;
mod man;
mod server;
mod store;
mod tools;
mod watcher;

use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

use crate::claude::ClaudeClient;
use crate::server::RustTutor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let claude = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(key) => {
            tracing::info!("ANTHROPIC_API_KEY set — reviews will use Claude API");
            Some(ClaudeClient::new(key))
        }
        Err(_) => {
            tracing::info!("No ANTHROPIC_API_KEY — reviews will be delegated to host LLM");
            None
        }
    };

    let tutor = RustTutor::new(claude)?;

    tracing::info!("Starting Rust Tutor MCP server");

    let service = tutor.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
