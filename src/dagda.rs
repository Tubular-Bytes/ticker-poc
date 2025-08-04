use std::collections::HashMap;

use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;
use tracing::{info, error, debug};

use crate::model;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Running,
    Stopping,
    Stopped,
}

pub enum Message {
    Task(model::Blueprint),
    Status(oneshot::Sender<Status>),
    Stop,
}

pub struct Dagda {
    pub bus: tokio::sync::mpsc::Receiver<Message>,
    pub broadcast: broadcast::Sender<BuildingSignal>,
    pub workers: HashMap<Uuid, Building>,
    status: Status,
    _keep_alive: broadcast::Receiver<BuildingSignal>, // Keep at least one receiver alive
}

impl Dagda {
    pub fn new(bus: tokio::sync::mpsc::Receiver<Message>) -> Self {
        let (broadcast, keep_alive) = broadcast::channel(32);
        Dagda { 
            bus, 
            broadcast, 
            workers: HashMap::new(), 
            status: Status::Stopped,
            _keep_alive: keep_alive,
        }
    }

    pub fn stop(&mut self) {
        self.status = Status::Stopping;
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        self.status = Status::Running;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        
        loop {
            if self.status != Status::Running {
                info!("Dagda stopping...");
                break;
            }

            tokio::select! {
                _ = ticker.tick() => {
                    // Handle tick event
                    debug!("Tick event received");
                    self.broadcast.send(BuildingSignal::Tick).unwrap_or_else(|e| {
                        error!("Failed to send tick signal: {}", e);
                        0
                    });
                }
                Some(message) = self.bus.recv() => {
                    match message {
                        Message::Task(blueprint) => {
                            info!("Received task: {:?}", blueprint);
                            
                            let building = Building::new(blueprint, self.broadcast.clone());

                            let id = Uuid::new_v4();
                            // Move the building out for the spawned task
                            let mut building = building;
                            self.workers.insert(id, building.clone());

                            tokio::spawn(async move {
                                if let Err(e) = building.run().await {
                                    error!("Error running building: {}", e);
                                }
                            });
                        }
                        Message::Stop => {
                            info!("Received stop message");
                            self.status = Status::Stopping;
                            break;
                        }
                        Message::Status(status_sender) => {
                            if let Err(e) = status_sender.send(self.status) {
                                error!("Failed to send status: {e:?}");
                            }
                        }
                    }
                }
            }
        }

        self.status = Status::Stopped;
        Ok(())
    } 
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildingStatus {
    Pending,
    InProgress,
    Paused,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildingSignal {
    Tick,
    Pause,
    Resume,
    Cancel
}

#[derive(Debug, Clone)]
pub struct Building {
    blueprint: model::Blueprint,
    status: BuildingStatus,
    progress: u64,
    ticks_tx: broadcast::Sender<BuildingSignal>
}

impl Building {
    pub fn new(blueprint: model::Blueprint, ticks_tx: broadcast::Sender<BuildingSignal>) -> Self {
            Building {
            blueprint,
            status: BuildingStatus::Pending,
            progress: 0,
            ticks_tx
        }
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        let mut ticks_rx = self.ticks_tx.subscribe();

        loop {
            match ticks_rx.recv().await? {
                BuildingSignal::Tick => {
                    if self.status == BuildingStatus::Pending {
                        self.status = BuildingStatus::InProgress;
                        info!("Building started for blueprint: {}", self.blueprint.id);
                    }

                    if self.status != BuildingStatus::InProgress {
                        continue;
                    }

                    self.progress += 1;
                    debug!("Building progress: {}", self.progress);
                    if self.progress >= self.blueprint.ticks {
                        self.status = BuildingStatus::Completed;
                        info!("Building completed for blueprint: {}", self.blueprint.id);
                        break;
                    }
                }
                BuildingSignal::Pause => {
                    self.status = BuildingStatus::Paused;
                    info!("Building paused for blueprint: {}", self.blueprint.id);
                }
                BuildingSignal::Resume => {
                    if self.status == BuildingStatus::Paused {
                        self.status = BuildingStatus::InProgress;
                        info!("Building resumed for blueprint: {}", self.blueprint.id);
                    }
                }
                BuildingSignal::Cancel => {
                    self.status = BuildingStatus::Cancelled;
                    info!("Building cancelled for blueprint: {}", self.blueprint.id);
                    break;
                }
            }
        }

        Ok(())
    }
}