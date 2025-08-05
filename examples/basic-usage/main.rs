use ticker_poc::dagda::controller::{self, Message, Status};
use ticker_poc::model::Blueprint;
use tracing::{error, info};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize tracing subscriber for stdout
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    info!("Starting ticker-poc application");

    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let mut dagda = controller::Dagda::new(rx);

    tokio::spawn(async move {
        if let Err(e) = dagda.run().await {
            error!("Error running Dagda: {}", e);
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    info!("Dagda status: {:?}", dagda_status(tx.clone()).await);

    let building_id = add_building(tx.clone()).await?;
    info!("Building added with ID: {}", building_id);

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let status = building_status(building_id, tx.clone()).await;
    info!("Building status: {:?}", status);

    // Pause building for a while
    if let Err(e) = pause_building(building_id, tx.clone()).await {
        error!("Failed to pause building: {}", e);
    } else {
        info!("Building paused successfully");
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let status = building_status(building_id, tx.clone()).await;
    info!("Building status: {:?}", status);

    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    if let Err(e) = resume_building(building_id, tx.clone()).await {
        error!("Failed to resume building: {}", e);
    } else {
        info!("Building resumed successfully");
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let status = building_status(building_id, tx.clone()).await;
    info!("Building status: {:?}", status);

    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
    info!("Stopping Dagda...");
    tx.send(controller::Message::Stop).await.unwrap();
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
) -> Result<controller::BuildingStatus, anyhow::Error> {
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
    let blueprint = Blueprint {
        id: "test-building-1".to_string(),
        ticks: 6,
    };
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(Message::Task(blueprint, reply_tx)).await.unwrap();
    let building_id = reply_rx.await.unwrap();
    info!("Building task started with ID: {}", building_id);
    Ok(building_id)
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
