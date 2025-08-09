use prometheus::{
    register_counter_vec, register_gauge_vec, register_histogram_vec, register_int_gauge_vec,
    CounterVec, Encoder, GaugeVec, HistogramVec, IntGaugeVec, TextEncoder,
};
use std::sync::OnceLock;
use sysinfo::System;
use tokio::sync::RwLock;
use std::collections::HashMap;

// Global metrics registry
pub static METRICS: OnceLock<Metrics> = OnceLock::new();

pub struct Metrics {
    // System metrics
    pub cpu_usage_percent: GaugeVec,
    pub memory_usage_bytes: GaugeVec,
    pub memory_total_bytes: GaugeVec,
    
    // Process-specific metrics for this application
    pub process_cpu_usage_percent: GaugeVec,
    pub process_memory_usage_bytes: GaugeVec,
    pub process_virtual_memory_bytes: GaugeVec,
    
    // Dagda metrics
    pub dagda_status: IntGaugeVec,
    pub shards_total: IntGaugeVec,
    pub workers_total: IntGaugeVec,
    pub workers_by_status: IntGaugeVec,
    
    // Worker lifecycle metrics
    pub worker_operations_total: CounterVec,
    pub worker_ticks_total: CounterVec,
    pub worker_progress_ratio: GaugeVec,
    
    // Backpressure metrics
    pub message_queue_size: IntGaugeVec,
    pub tick_processing_duration: HistogramVec,
    pub worker_processing_duration: HistogramVec,
    
    // HTTP metrics
    pub http_requests_total: CounterVec,
    pub http_request_duration: HistogramVec,
    
    // System info for periodic updates
    system: RwLock<System>,
}

impl Metrics {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let metrics = Self {
            // System metrics
            cpu_usage_percent: register_gauge_vec!(
                "system_cpu_usage_percent",
                "CPU usage percentage",
                &["cpu"]
            )?,
            memory_usage_bytes: register_gauge_vec!(
                "system_memory_usage_bytes",
                "Memory usage in bytes",
                &["type"]
            )?,
            memory_total_bytes: register_gauge_vec!(
                "system_memory_total_bytes", 
                "Total memory in bytes",
                &["type"]
            )?,
            
            // Process-specific metrics for this application
            process_cpu_usage_percent: register_gauge_vec!(
                "process_cpu_usage_percent",
                "CPU usage percentage of this process",
                &["pid"]
            )?,
            process_memory_usage_bytes: register_gauge_vec!(
                "process_memory_usage_bytes",
                "Memory usage of this process in bytes",
                &["type"]
            )?,
            process_virtual_memory_bytes: register_gauge_vec!(
                "process_virtual_memory_bytes",
                "Virtual memory usage of this process in bytes",
                &["pid"]
            )?,
            
            // Dagda metrics
            dagda_status: register_int_gauge_vec!(
                "dagda_status",
                "Dagda controller status (0=stopped, 1=running, 2=stopping)",
                &[]
            )?,
            shards_total: register_int_gauge_vec!(
                "dagda_shards_total",
                "Total number of shards",
                &[]
            )?,
            workers_total: register_int_gauge_vec!(
                "dagda_workers_total", 
                "Total number of workers",
                &["shard_id"]
            )?,
            workers_by_status: register_int_gauge_vec!(
                "dagda_workers_by_status",
                "Number of workers by status",
                &["status", "shard_id"]
            )?,
            
            // Worker lifecycle metrics
            worker_operations_total: register_counter_vec!(
                "dagda_worker_operations_total",
                "Total worker operations",
                &["operation", "status"]
            )?,
            worker_ticks_total: register_counter_vec!(
                "dagda_worker_ticks_total",
                "Total worker ticks processed",
                &["worker_id", "blueprint"]
            )?,
            worker_progress_ratio: register_gauge_vec!(
                "dagda_worker_progress_ratio",
                "Worker progress as a ratio (0.0 to 1.0)",
                &["worker_id", "blueprint"]
            )?,
            
            // Backpressure metrics
            message_queue_size: register_int_gauge_vec!(
                "dagda_message_queue_size",
                "Size of message queues",
                &["queue_type"]
            )?,
            tick_processing_duration: register_histogram_vec!(
                "dagda_tick_processing_duration_seconds",
                "Time spent processing tick events",
                &["shard_id"],
                prometheus::exponential_buckets(0.001, 2.0, 10)?
            )?,
            worker_processing_duration: register_histogram_vec!(
                "dagda_worker_processing_duration_seconds",
                "Time spent processing worker messages",
                &["message_type"],
                prometheus::exponential_buckets(0.001, 2.0, 10)?
            )?,
            
            // HTTP metrics
            http_requests_total: register_counter_vec!(
                "http_requests_total",
                "Total HTTP requests",
                &["method", "endpoint", "status"]
            )?,
            http_request_duration: register_histogram_vec!(
                "http_request_duration_seconds",
                "HTTP request duration",
                &["method", "endpoint"],
                prometheus::exponential_buckets(0.001, 2.0, 10)?
            )?,
            
            system: RwLock::new(System::new_all()),
        };
        
        Ok(metrics)
    }
    
    pub fn global() -> &'static Metrics {
        METRICS.get().expect("Metrics not initialized")
    }
    
    pub fn init() -> Result<(), Box<dyn std::error::Error>> {
        let metrics = Self::new()?;
        METRICS.set(metrics).map_err(|_| "Failed to initialize global metrics")?;
        Ok(())
    }
    
    // Update system metrics
    pub async fn update_system_metrics(&self) {
        let mut system = self.system.write().await;
        system.refresh_all();
        
        // CPU usage per core
        for (i, cpu) in system.cpus().iter().enumerate() {
            self.cpu_usage_percent
                .with_label_values(&[&format!("cpu{}", i)])
                .set(cpu.cpu_usage() as f64);
        }
        
        // Overall CPU usage - calculate average
        let total_cpu_usage = system.cpus().iter()
            .map(|cpu| cpu.cpu_usage())
            .sum::<f32>() / system.cpus().len() as f32;
        
        self.cpu_usage_percent
            .with_label_values(&["total"])
            .set(total_cpu_usage as f64);
        
        // Memory usage
        self.memory_usage_bytes
            .with_label_values(&["used"])
            .set(system.used_memory() as f64);
        
        self.memory_total_bytes
            .with_label_values(&["total"])
            .set(system.total_memory() as f64);
            
        self.memory_usage_bytes
            .with_label_values(&["available"])
            .set(system.available_memory() as f64);
            
        // Process-specific metrics for this application
        let current_pid = std::process::id();
        if let Some(process) = system.process(sysinfo::Pid::from_u32(current_pid)) {
            // Process CPU usage
            self.process_cpu_usage_percent
                .with_label_values(&[&current_pid.to_string()])
                .set(process.cpu_usage() as f64);
                
            // Process memory usage (RSS - Resident Set Size)
            self.process_memory_usage_bytes
                .with_label_values(&["rss"])
                .set(process.memory() as f64 * 1024.0); // Convert from KB to bytes
                
            // Process virtual memory usage
            self.process_virtual_memory_bytes
                .with_label_values(&[&current_pid.to_string()])
                .set(process.virtual_memory() as f64 * 1024.0); // Convert from KB to bytes
        }
    }
    
    // Update Dagda status
    pub fn update_dagda_status(&self, status: crate::dagda::controller::Status) {
        let status_value = match status {
            crate::dagda::controller::Status::Stopped => 0,
            crate::dagda::controller::Status::Running => 1,
            crate::dagda::controller::Status::Stopping => 2,
        };
        self.dagda_status.with_label_values(&[]).set(status_value);
    }
    
    // Update shard and worker counts
    pub fn update_shard_metrics(&self, shard_count: usize, workers_by_shard: &HashMap<String, usize>) {
        self.shards_total.with_label_values(&[]).set(shard_count as i64);
        
        for (shard_id, worker_count) in workers_by_shard {
            self.workers_total
                .with_label_values(&[shard_id])
                .set(*worker_count as i64);
        }
    }
    
    // Update worker status counts
    pub fn update_worker_status_metrics(&self, status_counts: &HashMap<(String, String), usize>) {
        // Clear previous metrics for all status/shard combinations
        for ((status, shard_id), count) in status_counts {
            self.workers_by_status
                .with_label_values(&[status, shard_id])
                .set(*count as i64);
        }
    }
    
    // Record worker operation
    pub fn record_worker_operation(&self, operation: &str, status: &str) {
        self.worker_operations_total
            .with_label_values(&[operation, status])
            .inc();
    }
    
    // Update worker progress
    pub fn update_worker_progress(&self, worker_id: &str, blueprint: &str, current: u64, total: u64) {
        let ratio = if total > 0 { current as f64 / total as f64 } else { 0.0 };
        
        self.worker_progress_ratio
            .with_label_values(&[worker_id, blueprint])
            .set(ratio);
            
        self.worker_ticks_total
            .with_label_values(&[worker_id, blueprint])
            .inc_by(1.0);
    }
    
    // Record message queue sizes
    pub fn update_message_queue_size(&self, queue_type: &str, size: usize) {
        self.message_queue_size
            .with_label_values(&[queue_type])
            .set(size as i64);
    }
    
    // Record tick processing time
    pub fn record_tick_processing_time(&self, shard_id: &str, duration: std::time::Duration) {
        self.tick_processing_duration
            .with_label_values(&[shard_id])
            .observe(duration.as_secs_f64());
    }
    
    // Record worker message processing time
    pub fn record_worker_message_processing_time(&self, message_type: &str, duration: std::time::Duration) {
        self.worker_processing_duration
            .with_label_values(&[message_type])
            .observe(duration.as_secs_f64());
    }
    
    // Record HTTP metrics
    pub fn record_http_request(&self, method: &str, endpoint: &str, status: u16, duration: std::time::Duration) {
        self.http_requests_total
            .with_label_values(&[method, endpoint, &status.to_string()])
            .inc();
            
        self.http_request_duration
            .with_label_values(&[method, endpoint])
            .observe(duration.as_secs_f64());
    }
    
    // Export metrics in Prometheus format
    pub fn export(&self) -> Result<String, Box<dyn std::error::Error>> {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }
}

// Background task to update system metrics periodically
pub async fn start_system_metrics_updater() {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    
    loop {
        interval.tick().await;
        if let Some(metrics) = METRICS.get() {
            metrics.update_system_metrics().await;
        }
    }
}
