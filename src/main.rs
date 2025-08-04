use ticker_poc::dagda::{self, Message, Status};
use ticker_poc::model::Blueprint;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize tracing subscriber for stdout
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    info!("Starting ticker-poc application");
    
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let mut dagda = dagda::Dagda::new(rx);

    tokio::spawn(async move {
        if let Err(e) = dagda.run().await {
            error!("Error running Dagda: {}", e);
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    info!("Dagda status: {:?}", dagda_status(tx.clone()).await);

    // Send a test task
    let blueprint = Blueprint {
        id: "test-building-1".to_string(),
        ticks: 5,
    };
    tx.send(Message::Task(blueprint)).await.unwrap();
    info!("Sent test task");

    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    info!("Stopping Dagda...");
    tx.send(dagda::Message::Stop).await.unwrap();    Ok(())
}

async fn dagda_status(tx: tokio::sync::mpsc::Sender<Message>) -> Result<Status, anyhow::Error> {
    let (status_tx, status_rx) = tokio::sync::oneshot::channel();
    tx.send(Message::Status(status_tx)).await?;
    status_rx.await.map_err(|e| anyhow::anyhow!("Failed to receive status: {}", e))
}
