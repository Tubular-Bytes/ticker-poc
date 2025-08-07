use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};
use uuid::Uuid;

use ticker_poc::dagda::controller::{Dagda, Message, Status as DagdaStatus};

// Request/Response types for the API
#[derive(Serialize)]
struct SystemStatusResponse {
    status: String,
    shards: usize,
}

#[derive(Serialize)]
struct WorkerResponse {
    id: Uuid,
    name: String,
    status: String,
    progress: ProgressInfo,
    parent: Option<Uuid>,
}

#[derive(Serialize)]
struct ProgressInfo {
    current_ticks: u64,
    total_ticks: u64,
    percentage: f64,
}

#[derive(Serialize)]
struct TaskCreatedResponse {
    worker_id: Uuid,
    message: String,
}

#[derive(Deserialize)]
struct CreateTaskQuery {
    ticks: u64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// Application state
#[derive(Clone)]
struct AppState {
    dagda_sender: mpsc::Sender<Message>,
    blueprints: ticker_poc::library::Collection,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting Ticker PoC server...");

    // Create Dagda controller with channels
    let (mut dagda, dagda_sender) = Dagda::with_channels(10); // max 10 workers per shard
    let blueprints = ticker_poc::library::Collection::load()?;

    // Spawn Dagda controller in background task
    tokio::spawn(async move {
        if let Err(e) = dagda.run().await {
            error!("Dagda controller error: {}", e);
        }
    });

    // Create application state
    let state = AppState {
        dagda_sender,
        blueprints,
    };

    // Build the router
    let app = Router::new()
        .route("/system/status", get(get_system_status))
        .route("/inventory", get(get_inventory))
        .route("/inventory/:name", post(create_task))
        .route("/inventory/:worker_id/start", put(start_worker))
        .route("/inventory/:worker_id/pause", put(pause_worker))
        .route("/inventory/:worker_id/resume", put(resume_worker))
        .route("/inventory/:worker_id/stop", put(stop_worker))
        .with_state(state);

    info!("Server starting on http://0.0.0.0:3000");

    // Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// Extract UUID from Authorization header
fn extract_auth_uuid(headers: &HeaderMap) -> Result<Uuid, (StatusCode, Json<ErrorResponse>)> {
    let auth_header = headers
        .get("authorization")
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Authorization header missing".to_string(),
                }),
            )
        })?
        .to_str()
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid authorization header".to_string(),
                }),
            )
        })?;

    // Remove "Bearer " prefix if present
    let uuid_str = auth_header.strip_prefix("Bearer ").unwrap_or("");

    Uuid::parse_str(uuid_str).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid UUID in authorization header".to_string(),
            }),
        )
    })
}

// GET /system/status - returns Dagda status
async fn get_system_status(
    State(state): State<AppState>,
) -> Result<Json<SystemStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (sender, receiver) = oneshot::channel();

    if state
        .dagda_sender
        .send(Message::Status(sender))
        .await
        .is_err()
    {
        error!("Failed to send status request to Dagda");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        ));
    }

    match receiver.await {
        Ok(status) => {
            let status_str = match status {
                DagdaStatus::Running => "running",
                DagdaStatus::Stopping => "stopping",
                DagdaStatus::Stopped => "stopped",
            };

            Ok(Json(SystemStatusResponse {
                status: status_str.to_string(),
                shards: 0, // TODO: Add shard count to Dagda status
            }))
        }
        Err(_) => {
            error!("Failed to receive status from Dagda");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            ))
        }
    }
}

// GET /inventory - returns all worker data for the authenticated parent
async fn get_inventory(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkerResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let parent_id = extract_auth_uuid(&headers)?;

    let (sender, receiver) = oneshot::channel();

    if state
        .dagda_sender
        .send(Message::WorkersByParent(parent_id, sender))
        .await
        .is_err()
    {
        error!("Failed to send workers request to Dagda");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        ));
    }

    match receiver.await {
        Ok(workers) => {
            let worker_responses: Vec<WorkerResponse> = workers
                .into_iter()
                .map(|worker| {
                    let percentage = if worker.total_ticks > 0 {
                        (worker.ticks as f64 / worker.total_ticks as f64) * 100.0
                    } else {
                        0.0
                    };

                    WorkerResponse {
                        id: worker.id,
                        name: worker.name,
                        status: format!("{}", worker.status),
                        progress: ProgressInfo {
                            current_ticks: worker.ticks,
                            total_ticks: worker.total_ticks,
                            percentage,
                        },
                        parent: worker.parent,
                    }
                })
                .collect();

            Ok(Json(worker_responses))
        }
        Err(_) => {
            error!("Failed to receive workers from Dagda");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            ))
        }
    }
}

// POST /inventory/{name}?ticks={ticks} - dispatches a message with the blueprint
async fn create_task(
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(params): Query<CreateTaskQuery>,
    State(state): State<AppState>,
) -> Result<Json<TaskCreatedResponse>, (StatusCode, Json<ErrorResponse>)> {
    let parent_id = extract_auth_uuid(&headers)?;

    if params.ticks == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Ticks must be greater than 0".to_string(),
            }),
        ));
    }

    let blueprint = state.blueprints.get_blueprint(&name).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Blueprint '{}' not found", name),
            }),
        )
    })?;

    let (sender, receiver) = oneshot::channel();

    if state
        .dagda_sender
        .send(Message::TaskWithParent(blueprint, parent_id, sender))
        .await
        .is_err()
    {
        error!("Failed to send task request to Dagda");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        ));
    }

    match receiver.await {
        Ok(worker_id) => {
            info!(
                "Created task '{}' with worker ID {} for parent {}",
                name, worker_id, parent_id
            );
            Ok(Json(TaskCreatedResponse {
                worker_id,
                message: format!("Task '{}' created successfully", name),
            }))
        }
        Err(_) => {
            error!("Failed to receive worker ID from Dagda");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            ))
        }
    }
}

// PUT /inventory/{worker-id}/start - starts a worker
async fn start_worker(
    headers: HeaderMap,
    Path(worker_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let parent_id = extract_auth_uuid(&headers)?;

    let (sender, receiver) = oneshot::channel();

    if state
        .dagda_sender
        .send(Message::WorkerStartWithParent(worker_id, parent_id, sender))
        .await
        .is_err()
    {
        error!("Failed to send start worker request to Dagda");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        ));
    }

    match receiver.await {
        Ok(Ok(())) => {
            info!("Started worker {} for parent {}", worker_id, parent_id);
            Ok(Json(serde_json::json!({
                "message": "Worker started successfully"
            })))
        }
        Ok(Err(error)) => {
            warn!("Failed to start worker {}: {}", worker_id, error);
            Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error })))
        }
        Err(_) => {
            error!("Failed to receive response from Dagda");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            ))
        }
    }
}

// PUT /inventory/{worker-id}/pause - pauses a worker
async fn pause_worker(
    headers: HeaderMap,
    Path(worker_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let parent_id = extract_auth_uuid(&headers)?;

    let (sender, receiver) = oneshot::channel();

    if state
        .dagda_sender
        .send(Message::WorkerPauseWithParent(worker_id, parent_id, sender))
        .await
        .is_err()
    {
        error!("Failed to send pause worker request to Dagda");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        ));
    }

    match receiver.await {
        Ok(Ok(())) => {
            info!("Paused worker {} for parent {}", worker_id, parent_id);
            Ok(Json(serde_json::json!({
                "message": "Worker paused successfully"
            })))
        }
        Ok(Err(error)) => {
            warn!("Failed to pause worker {}: {}", worker_id, error);
            Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error })))
        }
        Err(_) => {
            error!("Failed to receive response from Dagda");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            ))
        }
    }
}

// PUT /inventory/{worker-id}/resume - resumes a worker
async fn resume_worker(
    headers: HeaderMap,
    Path(worker_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let parent_id = extract_auth_uuid(&headers)?;

    let (sender, receiver) = oneshot::channel();

    if state
        .dagda_sender
        .send(Message::WorkerResumeWithParent(
            worker_id, parent_id, sender,
        ))
        .await
        .is_err()
    {
        error!("Failed to send resume worker request to Dagda");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        ));
    }

    match receiver.await {
        Ok(Ok(())) => {
            info!("Resumed worker {} for parent {}", worker_id, parent_id);
            Ok(Json(serde_json::json!({
                "message": "Worker resumed successfully"
            })))
        }
        Ok(Err(error)) => {
            warn!("Failed to resume worker {}: {}", worker_id, error);
            Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error })))
        }
        Err(_) => {
            error!("Failed to receive response from Dagda");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            ))
        }
    }
}

// PUT /inventory/{worker-id}/stop - stops a worker
async fn stop_worker(
    headers: HeaderMap,
    Path(worker_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let parent_id = extract_auth_uuid(&headers)?;

    let (sender, receiver) = oneshot::channel();

    if state
        .dagda_sender
        .send(Message::WorkerStopWithParent(worker_id, parent_id, sender))
        .await
        .is_err()
    {
        error!("Failed to send stop worker request to Dagda");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        ));
    }

    match receiver.await {
        Ok(Ok(())) => {
            info!("Stopped worker {} for parent {}", worker_id, parent_id);
            Ok(Json(serde_json::json!({
                "message": "Worker stopped successfully"
            })))
        }
        Ok(Err(error)) => {
            warn!("Failed to stop worker {}: {}", worker_id, error);
            Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error })))
        }
        Err(_) => {
            error!("Failed to receive response from Dagda");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            ))
        }
    }
}
