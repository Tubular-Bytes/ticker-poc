use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
};

use uuid::Uuid;

use crate::{dagda::controller::Tick, model};

#[derive(Debug, Clone)]
pub struct Worker {
    id: Uuid,
    blueprint: model::Blueprint,
    status: Status,
    ticks: u64,
}

impl Worker {
    pub fn new(blueprint: model::Blueprint) -> Self {
        Worker {
            id: Uuid::new_v4(),
            blueprint,
            status: Status::Idle,
            ticks: 0,
        }
    }

    pub fn status(&self) -> Status {
        self.status.clone()
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    pub fn progress(&self) -> (u64, u64) {
        (self.ticks, self.blueprint.ticks)
    }

    pub fn start(&mut self) {
        self.status = Status::InProgress;
        tracing::info!("Worker {} started", self.id);
    }

    pub fn stop(&mut self) {
        self.status = Status::Stopped;
        self.ticks = 0;
        tracing::info!("Worker {} stopped", self.id);
    }

    pub fn pause(&mut self) {
        self.status = Status::Paused;
        tracing::info!("Worker {} paused", self.id);
    }

    pub fn resume(&mut self) {
        self.status = Status::InProgress;
        tracing::info!("Worker {} resumed", self.id);
    }

    pub fn tick(&mut self) {
        if self.status == Status::InProgress {
            self.ticks += 1;

            if self.ticks >= self.blueprint.ticks {
                self.status = Status::Completed;
                tracing::info!("Worker {} completed", self.id);
            } else {
                tracing::debug!(
                    "Worker {} ticked: {}/{}",
                    self.id,
                    self.ticks,
                    self.blueprint.ticks
                );
            }
        } else if self.status != Status::Completed {
            tracing::warn!(
                "Worker {} is not in progress ({}), cannot tick",
                self.id,
                self.status
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    InProgress,
    Completed,
    Stopped,
    Paused,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Idle => write!(f, "Idle"),
            Status::InProgress => write!(f, "In Progress"),
            Status::Completed => write!(f, "Completed"),
            Status::Stopped => write!(f, "Stopped"),
            Status::Paused => write!(f, "Paused"),
        }
    }
}

#[derive(Debug)]
pub struct Shard {
    pub id: Uuid,
    pub workers: Arc<Mutex<HashMap<Uuid, Worker>>>,
    max_capacity: u64,
    register: Vec<Uuid>,
}

impl Shard {
    pub fn spawn(
        id: Uuid,
        sender: tokio::sync::broadcast::Sender<Tick>,
        max_capacity: u64,
    ) -> Self {
        let mut rx = sender.subscribe();

        let workers = Arc::new(Mutex::new(HashMap::new()));
        let s = Shard {
            id,
            workers: workers.clone(),
            max_capacity,
            register: Vec::new(),
        };

        let workers = s.workers.clone();

        tokio::spawn(async move {
            while let Ok(tick) = rx.recv().await {
                // Handle tick logic here
                tracing::debug!("Received tick: {:?}", tick);

                let mut workers = workers.lock().unwrap();

                for worker in workers.values_mut() {
                    worker.tick();
                }
            }
        });

        s
    }

    pub fn capacity(&self) -> u64 {
        self.register.len() as u64
    }

    pub fn dispatch(&mut self, blueprint: &model::Blueprint) -> Result<Uuid, String> {
        if self.capacity() >= self.max_capacity {
            return Err("Shard capacity exceeded".into());
        }

        let worker = Worker::new(blueprint.clone());
        let worker_id = worker.id;
        self.workers.lock().unwrap().insert(worker.id, worker);
        self.register.push(blueprint.id);

        Ok(worker_id)
    }
}
