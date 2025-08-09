use std::collections::HashMap;

use tokio::sync::{broadcast, oneshot};
use tracing::{debug, error, info};
use uuid::Uuid;
use std::time::Instant;

const TICK_RATE: u64 = 1;

use crate::{
    dagda::shard::{self, Status as WorkerStatus},
    library,
    metrics::Metrics,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Running,
    Stopping,
    Stopped,
}

#[derive(Debug)]
pub enum Message {
    Task(library::Blueprint, oneshot::Sender<Uuid>),
    TaskWithParent(library::Blueprint, Uuid, oneshot::Sender<Uuid>),
    Status(oneshot::Sender<Status>),
    Stop,

    WorkerStart(Uuid),
    WorkerPause(Uuid),
    WorkerResume(Uuid),
    WorkerCancel(Uuid),
    WorkerStatus(Uuid, oneshot::Sender<WorkerStatus>),
    WorkersByParent(Uuid, oneshot::Sender<Vec<WorkerInfo>>),

    // New operations that check parent ownership
    WorkerStartWithParent(Uuid, Uuid, oneshot::Sender<Result<(), String>>), // worker_id, parent_id, result
    WorkerPauseWithParent(Uuid, Uuid, oneshot::Sender<Result<(), String>>),
    WorkerResumeWithParent(Uuid, Uuid, oneshot::Sender<Result<(), String>>),
    WorkerStopWithParent(Uuid, Uuid, oneshot::Sender<Result<(), String>>),
}

#[derive(Debug)]
pub enum WorkerMessage {
    Completed(Uuid),
    Failed(Uuid, String),
    Progress(Uuid, u64, u64), // worker_id, current_ticks, total_ticks
}

#[derive(Debug, Clone)]
pub struct WorkerInfo {
    pub id: Uuid,
    pub name: String,
    pub status: WorkerStatus,
    pub ticks: u64,
    pub total_ticks: u64,
    pub parent: Option<Uuid>,
}

pub struct Dagda {
    max_shard_capacity: u64,
    pub inbox: tokio::sync::mpsc::Receiver<Message>,
    pub worker_inbox: tokio::sync::mpsc::Receiver<WorkerMessage>,
    pub worker_sender: tokio::sync::mpsc::Sender<WorkerMessage>,
    pub broadcast: broadcast::Sender<Tick>,
    status: Status,
    _keep_alive: broadcast::Receiver<Tick>, // Keep at least one receiver alive

    pub shards: HashMap<Uuid, shard::Shard>,
}

impl Dagda {
    pub fn new(inbox: tokio::sync::mpsc::Receiver<Message>, max_shard_capacity: u64) -> Self {
        let (broadcast, keep_alive) = broadcast::channel(32);
        let (worker_sender, worker_inbox) = tokio::sync::mpsc::channel(1000);

        Dagda {
            max_shard_capacity,
            inbox,
            worker_inbox,
            worker_sender,
            broadcast,
            status: Status::Stopped,
            _keep_alive: keep_alive,
            shards: HashMap::new(),
        }
    }

    /// Creates a new Dagda controller with the necessary channels
    /// Returns (Dagda instance, Message sender for external communication)
    pub fn with_channels(max_shard_capacity: u64) -> (Self, tokio::sync::mpsc::Sender<Message>) {
        let (sender, receiver) = tokio::sync::mpsc::channel(1000);
        let dagda = Self::new(receiver, max_shard_capacity);
        (dagda, sender)
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

        let shard = shard::Shard::spawn(
            shard_id,
            self.broadcast.clone(),
            self.worker_sender.clone(),
            self.max_shard_capacity,
        );
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

    pub async fn lookup_worker_mut(&mut self, worker_id: Uuid) -> Option<&mut shard::Shard> {
        self.shards
            .iter_mut()
            .find(|(_, shard)| shard.workers.lock().unwrap().contains_key(&worker_id))
            .map(|(_, shard)| shard)
    }

    pub async fn worker_belongs_to_parent(&self, worker_id: Uuid, parent_id: Uuid) -> bool {
        if let Some(shard) = self.lookup_worker(worker_id).await {
            if let Some(worker) = shard.workers.lock().unwrap().get(&worker_id) {
                return worker.parent() == Some(parent_id);
            }
        }
        false
    }

    // Update metrics for shards and workers
    async fn update_metrics(&self) {
        let metrics = Metrics::global();
        
        // Update shard count
        let shard_count = self.shards.len();
        
        // Collect worker counts by shard
        let mut workers_by_shard = HashMap::new();
        let mut status_counts = HashMap::new();
        
        // Update queue size metrics
        // Note: We can't easily get the exact queue sizes without additional instrumentation
        // but we can track the number of shards and workers as a proxy
        metrics.update_message_queue_size("dagda_inbox", shard_count);
        
        for (shard_id, shard) in &self.shards {
            let shard_id_str = shard_id.to_string();
            let workers = shard.workers.lock().unwrap();
            let worker_count = workers.len();
            
            workers_by_shard.insert(shard_id_str.clone(), worker_count);
            
            // Count workers by status for this shard
            let mut status_count_idle = 0;
            let mut status_count_in_progress = 0;
            let mut status_count_paused = 0;
            let mut status_count_stopped = 0;
            let mut status_count_completed = 0;
            
            for worker in workers.values() {
                match worker.status() {
                    WorkerStatus::Idle => status_count_idle += 1,
                    WorkerStatus::InProgress => status_count_in_progress += 1,
                    WorkerStatus::Paused => status_count_paused += 1,
                    WorkerStatus::Stopped => status_count_stopped += 1,
                    WorkerStatus::Completed => status_count_completed += 1,
                }
            }
            
            status_counts.insert(("idle".to_string(), shard_id_str.clone()), status_count_idle);
            status_counts.insert(("in_progress".to_string(), shard_id_str.clone()), status_count_in_progress);
            status_counts.insert(("paused".to_string(), shard_id_str.clone()), status_count_paused);
            status_counts.insert(("stopped".to_string(), shard_id_str.clone()), status_count_stopped);
            status_counts.insert(("completed".to_string(), shard_id_str), status_count_completed);
        }
        
        metrics.update_shard_metrics(shard_count, &workers_by_shard);
        metrics.update_worker_status_metrics(&status_counts);
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        self.status = Status::Running;
        let duration = self.interval();

        info!("Dagda started with tick rate: {} ms", duration.as_millis());

        // Update initial metrics
        let metrics = Metrics::global();
        metrics.update_dagda_status(self.status);

        let mut ticker = tokio::time::interval(duration);

        loop {
            if self.status != Status::Running {
                info!("Dagda stopping...");
                metrics.update_dagda_status(self.status);
                break;
            }

            tokio::select! {
                _ = ticker.tick() => {
                    let tick_start = Instant::now();
                    
                    // Handle tick event
                    debug!("Ticker fired, sending tick signal to all shards");
                    self.broadcast.send(Tick::new()).unwrap_or_else(|e| {
                        error!("Failed to send tick signal: {}", e);
                        0
                    });
                    
                    // Update metrics
                    self.update_metrics().await;
                    
                    let tick_duration = tick_start.elapsed();
                    metrics.record_tick_processing_time("all", tick_duration);
                }
                Some(worker_message) = self.worker_inbox.recv() => {
                    let processing_start = Instant::now();
                    
                    match worker_message {
                        WorkerMessage::Completed(worker_id) => {
                            info!("Worker {} completed its task", worker_id);
                            metrics.record_worker_operation("completed", "success");
                            metrics.record_worker_message_processing_time("completed", processing_start.elapsed());
                        }
                        WorkerMessage::Failed(worker_id, error) => {
                            error!("Worker {} failed: {}", worker_id, error);
                            metrics.record_worker_operation("failed", "error");
                            metrics.record_worker_message_processing_time("failed", processing_start.elapsed());
                        }
                        WorkerMessage::Progress(worker_id, current, total) => {
                            debug!("Worker {} progress: {}/{}", worker_id, current, total);
                            
                            // Find the worker and get its blueprint for metrics
                            if let Some(shard) = self.lookup_worker(worker_id).await {
                                if let Some(worker) = shard.workers.lock().unwrap().get(&worker_id) {
                                    metrics.update_worker_progress(
                                        &worker_id.to_string(),
                                        &worker.blueprint().slug(),
                                        current,
                                        total
                                    );
                                    metrics.record_worker_message_processing_time("progress", processing_start.elapsed());
                                }
                            }
                        }
                    }
                }
                Some(message) = self.inbox.recv() => {
                    let processing_start = Instant::now();
                    
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
                                            metrics.record_worker_operation("task_created", "success");
                                            break;
                                        }
                                        Err(e) => {
                                            error!("Failed to dispatch task to shard {}: {}", shard_id, e);
                                            metrics.record_worker_operation("task_created", "error");
                                        }
                                    }
                                }
                            }

                            sender.send(final_id).unwrap_or_else(|e| {
                                error!("Failed to send task response: {}", e);
                            });
                            
                            metrics.record_worker_message_processing_time("task", processing_start.elapsed());
                        },
                        Message::TaskWithParent(blueprint, parent, sender) => {
                            let final_id: Uuid;
                            loop {
                                let shard_id = self.pick_shard().await;
                                if let Some(shard) = self.shards.get_mut(&shard_id) {
                                    match shard.dispatch_with_parent(&blueprint, parent) {
                                        Ok(worker_id) => {
                                            final_id = worker_id;
                                            info!("Dispatched task with parent {} to shard {} with worker ID {}", parent, shard_id, worker_id);
                                            metrics.record_worker_operation("task_with_parent_created", "success");
                                            break;
                                        }
                                        Err(e) => {
                                            error!("Failed to dispatch task to shard {}: {}", shard_id, e);
                                            metrics.record_worker_operation("task_with_parent_created", "error");
                                        }
                                    }
                                }
                            }

                            sender.send(final_id).unwrap_or_else(|e| {
                                error!("Failed to send task response: {}", e);
                            });
                            
                            metrics.record_worker_message_processing_time("task_with_parent", processing_start.elapsed());
                        },
                        Message::WorkersByParent(parent_id, sender) => {
                            let mut workers_info = Vec::new();

                            for shard in self.shards.values() {
                                let workers = shard.workers.lock().unwrap();
                                for worker in workers.values() {
                                    if worker.parent() == Some(parent_id) {
                                        let (current_ticks, total_ticks) = worker.progress();
                                        workers_info.push(WorkerInfo {
                                            id: worker.id(),
                                            name: worker.blueprint().slug().clone(),
                                            status: worker.status(),
                                            ticks: current_ticks,
                                            total_ticks,
                                            parent: worker.parent(),
                                        });
                                    }
                                }
                            }

                            sender.send(workers_info).unwrap_or_else(|_| {
                                error!("Failed to send workers by parent response");
                            });
                            
                            metrics.record_worker_message_processing_time("workers_by_parent", processing_start.elapsed());
                        },
                        Message::Status(sender) => {
                            if sender.send(self.status).is_err() {
                                error!("Failed to send status");
                            }
                            metrics.record_worker_message_processing_time("status", processing_start.elapsed());
                        },
                        Message::Stop => {
                            self.stop();
                            info!("Dagda received stop signal");
                            metrics.record_worker_message_processing_time("stop", processing_start.elapsed());
                        },
                        Message::WorkerStart(uuid) => {
                            if let Some(shard) = self.lookup_worker(uuid).await {
                                shard.workers.lock().unwrap().get_mut(&uuid).map(|worker| {
                                    worker.start();
                                    info!("Started worker {}", uuid);
                                    metrics.record_worker_operation("worker_start", "success");
                                }).unwrap_or_else(|| {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                    metrics.record_worker_operation("worker_start", "error");
                                });
                            } else {
                                error!("Shard for worker {} not found", uuid);
                                metrics.record_worker_operation("worker_start", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_start", processing_start.elapsed());
                        },
                        Message::WorkerPause(uuid) => {
                            if let Some(shard) = self.shards.iter_mut().find(|(_, shard)| {
                                shard.workers.lock().unwrap().contains_key(&uuid)
                            }).map(|(_, shard)| shard) {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&uuid) {
                                    worker.pause();
                                    info!("Paused worker {}", uuid);
                                    metrics.record_worker_operation("worker_pause", "success");
                                } else {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                    metrics.record_worker_operation("worker_pause", "error");
                                }
                            } else {
                                error!("Shard for worker {} not found", uuid);
                                metrics.record_worker_operation("worker_pause", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_pause", processing_start.elapsed());
                        },
                        Message::WorkerResume(uuid) => {
                            if let Some(shard) = self.shards.iter_mut().find(|(_, shard)| {
                                shard.workers.lock().unwrap().contains_key(&uuid)
                            }).map(|(_, shard)| shard) {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&uuid) {
                                    worker.resume();
                                    info!("Resumed worker {}", uuid);
                                    metrics.record_worker_operation("worker_resume", "success");
                                } else {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                    metrics.record_worker_operation("worker_resume", "error");
                                }
                            } else {
                                error!("Shard for worker {} not found", uuid);
                                metrics.record_worker_operation("worker_resume", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_resume", processing_start.elapsed());
                        },
                        Message::WorkerCancel(uuid) => {
                            if let Some(shard) = self.shards.iter_mut().find(|(_, shard)| {
                                shard.workers.lock().unwrap().contains_key(&uuid)
                            }).map(|(_, shard)| shard) {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&uuid) {
                                    worker.stop();
                                    info!("Cancelled worker {}", uuid);
                                    metrics.record_worker_operation("worker_cancel", "success");
                                } else {
                                    error!("Worker {} not found in shard {}", uuid, shard.id);
                                    metrics.record_worker_operation("worker_cancel", "error");
                                }
                            } else {
                                error!("Shard for worker {} not found", uuid);
                                metrics.record_worker_operation("worker_cancel", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_cancel", processing_start.elapsed());
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
                            metrics.record_worker_message_processing_time("worker_status", processing_start.elapsed());
                        },
                        Message::WorkerStartWithParent(worker_id, parent_id, sender) => {
                            if !self.worker_belongs_to_parent(worker_id, parent_id).await {
                                let _ = sender.send(Err("Worker not found or not owned by parent".to_string()));
                                metrics.record_worker_operation("worker_start_with_parent", "unauthorized");
                            } else if let Some(shard) = self.lookup_worker_mut(worker_id).await {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&worker_id) {
                                    worker.start();
                                    info!("Started worker {} for parent {}", worker_id, parent_id);
                                    let _ = sender.send(Ok(()));
                                    metrics.record_worker_operation("worker_start_with_parent", "success");
                                } else {
                                    let _ = sender.send(Err("Worker not found".to_string()));
                                    metrics.record_worker_operation("worker_start_with_parent", "error");
                                }
                            } else {
                                let _ = sender.send(Err("Shard not found".to_string()));
                                metrics.record_worker_operation("worker_start_with_parent", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_start_with_parent", processing_start.elapsed());
                        },
                        Message::WorkerPauseWithParent(worker_id, parent_id, sender) => {
                            if !self.worker_belongs_to_parent(worker_id, parent_id).await {
                                let _ = sender.send(Err("Worker not found or not owned by parent".to_string()));
                                metrics.record_worker_operation("worker_pause_with_parent", "unauthorized");
                            } else if let Some(shard) = self.lookup_worker_mut(worker_id).await {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&worker_id) {
                                    worker.pause();
                                    info!("Paused worker {} for parent {}", worker_id, parent_id);
                                    let _ = sender.send(Ok(()));
                                    metrics.record_worker_operation("worker_pause_with_parent", "success");
                                } else {
                                    let _ = sender.send(Err("Worker not found".to_string()));
                                    metrics.record_worker_operation("worker_pause_with_parent", "error");
                                }
                            } else {
                                let _ = sender.send(Err("Shard not found".to_string()));
                                metrics.record_worker_operation("worker_pause_with_parent", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_pause_with_parent", processing_start.elapsed());
                        },
                        Message::WorkerResumeWithParent(worker_id, parent_id, sender) => {
                            if !self.worker_belongs_to_parent(worker_id, parent_id).await {
                                let _ = sender.send(Err("Worker not found or not owned by parent".to_string()));
                                metrics.record_worker_operation("worker_resume_with_parent", "unauthorized");
                            } else if let Some(shard) = self.lookup_worker_mut(worker_id).await {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&worker_id) {
                                    worker.resume();
                                    info!("Resumed worker {} for parent {}", worker_id, parent_id);
                                    let _ = sender.send(Ok(()));
                                    metrics.record_worker_operation("worker_resume_with_parent", "success");
                                } else {
                                    let _ = sender.send(Err("Worker not found".to_string()));
                                    metrics.record_worker_operation("worker_resume_with_parent", "error");
                                }
                            } else {
                                let _ = sender.send(Err("Shard not found".to_string()));
                                metrics.record_worker_operation("worker_resume_with_parent", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_resume_with_parent", processing_start.elapsed());
                        },
                        Message::WorkerStopWithParent(worker_id, parent_id, sender) => {
                            if !self.worker_belongs_to_parent(worker_id, parent_id).await {
                                let _ = sender.send(Err("Worker not found or not owned by parent".to_string()));
                                metrics.record_worker_operation("worker_stop_with_parent", "unauthorized");
                            } else if let Some(shard) = self.lookup_worker_mut(worker_id).await {
                                if let Some(worker) = shard.workers.lock().unwrap().get_mut(&worker_id) {
                                    worker.stop();
                                    info!("Stopped worker {} for parent {}", worker_id, parent_id);
                                    let _ = sender.send(Ok(()));
                                    metrics.record_worker_operation("worker_stop_with_parent", "success");
                                } else {
                                    let _ = sender.send(Err("Worker not found".to_string()));
                                    metrics.record_worker_operation("worker_stop_with_parent", "error");
                                }
                            } else {
                                let _ = sender.send(Err("Shard not found".to_string()));
                                metrics.record_worker_operation("worker_stop_with_parent", "error");
                            }
                            metrics.record_worker_message_processing_time("worker_stop_with_parent", processing_start.elapsed());
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
    use crate::library::{Blueprint, Building};

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

        let blueprint = Blueprint::Building(Building {
            slug: "test-building-1".to_string(),
            name: "Test Building 1".to_string(),
            version: "v1.0.0".into(),
            ticks: 6,
            cost: HashMap::new(),
            attributes: HashMap::new(),
        });

        let shard_1 = dagda.shards.get_mut(&shard_id_1).unwrap();
        shard_1.dispatch(&blueprint).unwrap();

        let picked_shard_id = dagda.pick_shard().await;
        assert!(picked_shard_id == shard_id_2);
    }

    #[tokio::test]
    async fn test_dagda_pick_shard_multiple_shards_none_available() {
        let mut dagda = Dagda::new(tokio::sync::mpsc::channel(32).1, 1);
        let shard_id_1 = dagda.add_shard();
        let shard_id_2 = dagda.add_shard();

        let blueprint_1 = Blueprint::Building(Building {
            slug: "test-building-1".to_string(),
            name: "Test Building 1".to_string(),
            version: "v1.0.0".into(),
            ticks: 6,
            cost: HashMap::new(),
            attributes: HashMap::new(),
        });
        let blueprint_2 = Blueprint::Building(Building {
            slug: "test-building-2".to_string(),
            name: "Test Building 2".to_string(),
            version: "v1.0.0".into(),
            ticks: 6,
            cost: HashMap::new(),
            attributes: HashMap::new(),
        });

        let shard_1 = dagda.shards.get_mut(&shard_id_1).unwrap();
        shard_1.dispatch(&blueprint_1).unwrap();

        let shard_2 = dagda.shards.get_mut(&shard_id_2).unwrap();
        shard_2.dispatch(&blueprint_2).unwrap();

        let picked_shard_id = dagda.pick_shard().await;
        assert!(picked_shard_id != shard_id_1 && picked_shard_id != shard_id_2);
    }
}
