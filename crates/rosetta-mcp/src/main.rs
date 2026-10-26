//! Standalone MCP entry point for hosts that require a dedicated command.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rosetta_mcp::serve(std::env::current_dir()?).await
}
