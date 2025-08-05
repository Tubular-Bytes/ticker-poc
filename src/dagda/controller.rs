use std::collections::HashMap;

use tokio::sync::{broadcast, oneshot};
use tracing::{debug, error, info};
use uuid::Uuid;

const TICK_RATE: u64 = 64;

use crate::{
    dagda::shard::{self, Status as WorkerStatus},
    model,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Running,
    Stopping,
    Stopped,
}

#[derive(Debug)]
pub enum Message {
    Task(model::Blueprint, oneshot::Sender<Uuid>),
    Status(oneshot::Sender<Status>),
    Stop,

    WorkerStart(Uuid),
    WorkerPause(Uuid),
    WorkerResume(Uuid),
    WorkerCancel(Uuid),
    WorkerStatus(Uuid, oneshot::Sender<WorkerStatus>),
}

pub struct Dagda {
    max_shard_capacity: u64,
    pub bus: tokio::sync::mpsc::Receiver<Message>,
    pub broadcast: broadcast::Sender<Tick>,
    status: Status,
    _keep_alive: broadcast::Receiver<Tick>, // Keep at least one receiver alive

    pub shards: HashMap<Uuid, shard::Shard>,
}

impl Dagda {
    pub fn new(bus: tokio::sync::mpsc::Receiver<Message>, max_shard_capacity: u64) -> Self {
        let (broadcast, keep_alive) = broadcast::channel(32);
        Dagda {
            max_shard_capacity,
            bus,
            broadcast,
            status: Status::Stopped,
            _keep_alive: keep_alive,
            shards: HashMap::new(),
        }
    }

    pub fn stop(&mut self) {
        self.status = Status::Stopping;
    }

    pub fn interval(&self) -> tokio::time::Duration {
        let millis = 1000 / TICK_RATE;

        std::time::Duration::from_millis(millis)
    }

    pub fn add_shard(&mut self) -> Uuid {
        let shard_id = Uuid::new_v4();

        let shard = shard::Shard::spawn(shard_id, self.broadcast.clone(), self.max_shard_capacity);
        self.shards.insert(shard_id, shard);
        shard_id
    }

    pub async fn pick_shard(&mut self) -> Uuid {
        if self.shards.is_empty() {
            return self.add_shard();
        }

        match self
            .shards
            .iter()
            .find(|(_, shard)| shard.capacity() < self.max_shard_capacity)
        {
            Some(available_shard) => *available_shard.0,
            None => self.add_shard(),
        }
    }

    pub async fn lookup_worker(&self, worker_id: Uuid) -> Option<&shard::Shard> {
        self.shards
            .iter()
            .find(|(_, shard)| shard.workers.lock().unwrap().contains_key(&worker_id))
            .map(|(_, shard)| shard)
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        self.status = Status::Running;
        let duration = self.interval();

        info!("Dagda started with tick rate: {} ms", duration.as_millis());

        let mut ticker = tokio::time::interval(duration);

        loop {
            if self.status != Status::Running {
                info!("Dagda stopping...");
                break;
            }

            tokio::select! {
                _ = ticker.tick() => {
                    // Handle tick event
                    debug!("Ticker fired, sending tick signal to all shards");
                    self.broadcast.send(Tick::new()).unwrap_or_else(|e| {
                        error!("Failed to send tick signal: {}", e);
                        0
                    });
                }
                Some(message) = self.bus.recv() => {
                    match message {
                        Message::Task(blueprint, sender) => {
                            let final_id: Uuid;
                            loop {
                                let shard_id = self.pick_shard().await;
                                if let Some(shard) = self.shards.get_mut(&shard_id) {
                                    match shard.dispatch(&blueprint) {
                                        Ok(worker_id) => {
                                            final_id = worker_id;
                                            info!("Dispatched task to shard {} with worker ID {}", shard_id, worker_id);
                                            break;
                                        }
                                        Err(e) => {
                                            error!("Failed to dispatch task to shard {}: {}", shard_id, e);
                                        }
                                    }
                                }
                            }

                            sender.send(final_id).unwrap_or_else(|e| {
                                error!("Failed to send task response: {}", e);
                            });
                        },
                        Message::Status(sender) => {
                            if sender.send(self.status).is_err() {
                                error!("Failed to send status");
                            }
                        },
                        Message::Stop => {
                            self.stop();
                            info!("Dagda received stop signal");
                        },
                        Message::WorkerStart(uuid) => {
                            if let Some(shard) = self.lookup_worker(uuid).await {
                                shard.workers.lock().unwrap().get_mut(&uuid).map(|worker| {
                                    worker.start();
                                    info!("Started worker {}", uuid);
                                }).unwrap_or_else(|| {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                });
                            } else {
                                error!("Shard for worker {} not found", uuid);
                            }
                        },
                        Message::WorkerPause(uuid) => {
                            if let Some(shard) = self.shards.iter_mut().find(|(_, shard)| {
                                shard.workers.lock().unwrap().contains_key(&uuid)
                            }).map(|(_, shard)| shard) {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&uuid) {
                                    worker.pause();
                                    info!("Paused worker {}", uuid);
                                } else {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                }
                            } else {
                                error!("Shard for worker {} not found", uuid);
                            }
                        },
                        Message::WorkerResume(uuid) => {
                            if let Some(shard) = self.shards.iter_mut().find(|(_, shard)| {
                                shard.workers.lock().unwrap().contains_key(&uuid)
                            }).map(|(_, shard)| shard) {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&uuid) {
                                    worker.resume();
                                    info!("Resumed worker {}", uuid);
                                } else {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                }
                            } else {
                                error!("Shard for worker {} not found", uuid);
                            }
                        },
                        Message::WorkerCancel(uuid) => {
                            if let Some(shard) = self.shards.iter_mut().find(|(_, shard)| {
                                shard.workers.lock().unwrap().contains_key(&uuid)
                            }).map(|(_, shard)| shard) {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&uuid) {
                                    worker.stop();
                                    info!("Cancelled worker {}", uuid);
                                } else {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                }
                            } else {
                                error!("Shard for worker {} not found", uuid);
                            }
                        },
                        Message::WorkerStatus(uuid, sender) => {
                            if let Some(shard) = self.lookup_worker(uuid).await {
                                if let Some(worker) = shard.workers.lock().unwrap().get(&uuid) {
                                    let status = worker.status();
                                    if sender.send(status).is_err() {
                                        error!("Failed to send worker status for {}", uuid);
                                    }
                                } else {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                    sender.send(WorkerStatus::Stopped).unwrap_or_else(|e| {
                                        error!("Failed to send worker status: {}", e);
                                    });
                                }
                            } else {
                                error!("Shard for worker {} not found", uuid);
                                sender.send(WorkerStatus::Stopped).unwrap_or_else(|e| {
                                    error!("Failed to send worker status: {}", e);
                                });
                            }
                        },
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

#[derive(Debug, Clone)]
pub struct Tick {
    _timestamp: chrono::DateTime<chrono::Utc>,
}

impl Tick {
    pub fn new() -> Self {
        Tick {
            _timestamp: chrono::Utc::now(),
        }
    }
}

impl Default for Tick {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Blueprint;

    use super::*;

    #[tokio::test]
    async fn test_dagda_add_shard() {
        let mut dagda = Dagda::new(tokio::sync::mpsc::channel(32).1, 1);
        let shard_id = dagda.add_shard();
        assert!(dagda.shards.contains_key(&shard_id));
    }

    #[tokio::test]
    async fn test_dagda_pick_shard_empty() {
        let mut dagda = Dagda::new(tokio::sync::mpsc::channel(32).1, 1);
        assert!(dagda.shards.is_empty());
        let shard_id = dagda.pick_shard().await;
        assert!(dagda.shards.len() == 1);
        assert!(dagda.shards.contains_key(&shard_id));
    }

    #[tokio::test]
    async fn test_dagda_pick_shard_one_shard() {
        let mut dagda = Dagda::new(tokio::sync::mpsc::channel(32).1, 1);
        let shard_id = dagda.add_shard();
        let picked_shard_id = dagda.pick_shard().await;
        assert_eq!(picked_shard_id, shard_id);
    }

    #[tokio::test]
    async fn test_dagda_pick_shard_multiple_shards() {
        let mut dagda = Dagda::new(tokio::sync::mpsc::channel(32).1, 1);
        let shard_id_1 = dagda.add_shard();
        let shard_id_2 = dagda.add_shard();

        let shard_1 = dagda.shards.get_mut(&shard_id_1).unwrap();
        shard_1
            .dispatch(&Blueprint::new("test-building-1".to_string(), 6))
            .unwrap();

        let picked_shard_id = dagda.pick_shard().await;
        assert!(picked_shard_id == shard_id_2);
    }

    #[tokio::test]
    async fn test_dagda_pick_shard_multiple_shards_none_available() {
        let mut dagda = Dagda::new(tokio::sync::mpsc::channel(32).1, 1);
        let shard_id_1 = dagda.add_shard();
        let shard_id_2 = dagda.add_shard();

        let shard_1 = dagda.shards.get_mut(&shard_id_1).unwrap();
        shard_1
            .dispatch(&Blueprint::new("test-building-1".to_string(), 6))
            .unwrap();

        let shard_2 = dagda.shards.get_mut(&shard_id_2).unwrap();
        shard_2
            .dispatch(&Blueprint::new("test-building-1".to_string(), 6))
            .unwrap();

        let picked_shard_id = dagda.pick_shard().await;
        assert!(picked_shard_id != shard_id_1 && picked_shard_id != shard_id_2);
    }
}
