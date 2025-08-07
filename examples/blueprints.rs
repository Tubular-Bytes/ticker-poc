use ticker_poc::library;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let blueprints = library::BlueprintCollection::load()?;

    let keys = blueprints.blueprints().keys().collect::<Vec<_>>();
    info!(?keys, "Loaded blueprints");

    Ok(())
}
