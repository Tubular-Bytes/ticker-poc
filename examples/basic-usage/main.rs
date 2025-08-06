use ticker_poc::dagda::controller::{self, Message, Status};
use ticker_poc::dagda::shard;
use ticker_poc::model::Blueprint;
use tracing::{error, info};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize tracing subscriber for stdout
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting ticker-poc application");

    let (mut dagda, tx) = controller::Dagda::with_channels(10);

    tokio::spawn(async move {
        if let Err(e) = dagda.run().await {
            error!("Error running Dagda: {}", e);
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let status = dagda_status(tx.clone()).await?;
    info!("Dagda status: {:?}", status);

    let building_id = add_building(tx.clone()).await?;
    start_building(building_id, tx.clone()).await?;

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    pause_building(building_id, tx.clone()).await?;

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let status = building_status(building_id, tx.clone()).await?;
    info!("Building {building_id} status: {:?}", status);

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    resume_building(building_id, tx.clone()).await?;

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let status = building_status(building_id, tx.clone()).await?;
    info!("Building {building_id} status: {:?}", status);

    loop {
        match building_status(building_id, tx.clone()).await {
            Ok(status) => {
                if status == shard::Status::Completed {
                    info!("Building {building_id} completed");
                    break;
                }
            }
            Err(e) => {
                error!("Error getting building status: {}", e);
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(())
}

async fn dagda_status(tx: tokio::sync::mpsc::Sender<Message>) -> Result<Status, anyhow::Error> {
    let (status_tx, status_rx) = tokio::sync::oneshot::channel();
    tx.send(Message::Status(status_tx)).await?;
    status_rx
        .await
        .map_err(|e| anyhow::anyhow!("Failed to receive status: {}", e))
}

async fn building_status(
    building_id: Uuid,
    tx: tokio::sync::mpsc::Sender<Message>,
) -> Result<shard::Status, anyhow::Error> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(Message::WorkerStatus(building_id, reply_tx))
        .await
        .unwrap();
    reply_rx
        .await
        .map_err(|e| anyhow::anyhow!("Failed to receive building status: {}", e))
}

async fn add_building(tx: tokio::sync::mpsc::Sender<Message>) -> Result<Uuid, anyhow::Error> {
    // Send a test task
    let blueprint = Blueprint::new("test-building-1".to_string(), 6);
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(Message::Task(blueprint, reply_tx)).await.unwrap();
    let building_id = reply_rx.await.unwrap();
    info!("Building task started with ID: {}", building_id);
    Ok(building_id)
}

async fn start_building(
    building_id: Uuid,
    tx: tokio::sync::mpsc::Sender<Message>,
) -> Result<(), anyhow::Error> {
    tx.send(Message::WorkerStart(building_id)).await?;
    Ok(())
}

async fn pause_building(
    building_id: Uuid,
    tx: tokio::sync::mpsc::Sender<Message>,
) -> Result<(), anyhow::Error> {
    tx.send(Message::WorkerPause(building_id)).await?;
    Ok(())
}

async fn resume_building(
    building_id: Uuid,
    tx: tokio::sync::mpsc::Sender<Message>,
) -> Result<(), anyhow::Error> {
    tx.send(Message::WorkerResume(building_id)).await?;
    Ok(())
}
