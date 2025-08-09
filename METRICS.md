# Prometheus Metrics Integration

This document describes the Prometheus metrics implementation added to the Ticker PoC application for monitoring backpressure, memory usage, CPU usage, and worker/shard statistics.

## Overview

The application now exposes comprehensive metrics at the `/metrics` endpoint in Prometheus format, allowing for monitoring and alerting on system performance and application behavior.

## Metrics Categories

### System Metrics

- **`system_cpu_usage_percent`** - CPU usage percentage per core and total
  - Labels: `cpu` (cpu0, cpu1, ..., total)
  - Type: Gauge
  - Description: Real-time CPU usage across all cores

- **`system_memory_usage_bytes`** - Memory usage in bytes  
  - Labels: `type` (used, available)
  - Type: Gauge
  - Description: Current memory utilization

- **`system_memory_total_bytes`** - Total system memory
  - Labels: `type` (total)
  - Type: Gauge
  - Description: Total available system memory

### Dagda Controller Metrics

- **`dagda_status`** - Controller status
  - Values: 0=stopped, 1=running, 2=stopping
  - Type: Gauge
  - Description: Current state of the Dagda controller

- **`dagda_shards_total`** - Total number of shards
  - Type: Gauge
  - Description: Number of active shards in the system

- **`dagda_workers_total`** - Total workers per shard
  - Labels: `shard_id`
  - Type: Gauge
  - Description: Number of workers in each shard

- **`dagda_workers_by_status`** - Workers grouped by status
  - Labels: `status` (idle, in_progress, completed, paused, stopped), `shard_id`
  - Type: Gauge
  - Description: Distribution of workers across different states

### Worker Lifecycle Metrics

- **`dagda_worker_operations_total`** - Total worker operations
  - Labels: `operation` (completed, failed, worker_start, etc.), `status` (success, error, unauthorized)
  - Type: Counter
  - Description: Cumulative count of worker operations and their outcomes

- **`dagda_worker_ticks_total`** - Total worker ticks processed
  - Labels: `worker_id`, `blueprint`
  - Type: Counter
  - Description: Number of ticks processed by each worker

- **`dagda_worker_progress_ratio`** - Worker progress as ratio
  - Labels: `worker_id`, `blueprint`
  - Type: Gauge
  - Range: 0.0 to 1.0
  - Description: Completion percentage for each worker

### Backpressure and Performance Metrics

- **`dagda_message_queue_size`** - Message queue sizes
  - Labels: `queue_type` (dagda_inbox, worker_inbox)
  - Type: Gauge
  - Description: Current size of message queues (proxy for backpressure)

- **`dagda_tick_processing_duration_seconds`** - Tick processing time
  - Labels: `shard_id`
  - Type: Histogram
  - Description: Time spent processing tick events

- **`dagda_worker_processing_duration_seconds`** - Worker message processing time
  - Labels: `message_type` (completed, failed, progress, etc.)
  - Type: Histogram
  - Description: Time spent processing different types of worker messages

### HTTP Metrics

- **`http_requests_total`** - Total HTTP requests
  - Labels: `method`, `endpoint`, `status`
  - Type: Counter
  - Description: Count of HTTP requests by method, endpoint, and status code

- **`http_request_duration_seconds`** - HTTP request duration
  - Labels: `method`, `endpoint`
  - Type: Histogram
  - Description: Response time distribution for HTTP requests

## Usage

### Accessing Metrics

The metrics are available at the `/metrics` endpoint:

```bash
curl http://localhost:3001/metrics
```

### Example Prometheus Configuration

Add this to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'ticker-poc'
    static_configs:
      - targets: ['localhost:3001']
    metrics_path: '/metrics'
    scrape_interval: 5s
```

### Example Grafana Queries

**System CPU Usage:**
```promql
system_cpu_usage_percent{cpu="total"}
```

**Worker Status Distribution:**
```promql
sum by (status) (dagda_workers_by_status)
```

**HTTP Request Rate:**
```promql
rate(http_requests_total[5m])
```

**Worker Progress:**
```promql
dagda_worker_progress_ratio
```

**Message Processing Latency:**
```promql
histogram_quantile(0.95, rate(dagda_worker_processing_duration_seconds_bucket[5m]))
```

### Alerting Examples

**High CPU Usage:**
```promql
system_cpu_usage_percent{cpu="total"} > 80
```

**Worker Failures:**
```promql
rate(dagda_worker_operations_total{status="error"}[5m]) > 0.1
```

**High Message Processing Latency:**
```promql
histogram_quantile(0.95, rate(dagda_worker_processing_duration_seconds_bucket[5m])) > 0.1
```

## Implementation Details

### Metrics Collection

- **System metrics** are updated every 5 seconds by a background task
- **Dagda metrics** are updated on every tick (1 second intervals)
- **Worker metrics** are updated in real-time as operations occur
- **HTTP metrics** are collected via middleware on every request

### Architecture

- Global metrics registry using `OnceLock<Metrics>`
- Prometheus client library for metric types and encoding
- System information via `sysinfo` crate
- HTTP middleware for request/response metrics
- Integrated into Dagda controller's main event loop

### Performance Considerations

- Metrics collection has minimal overhead
- Uses efficient Prometheus client library
- System metrics limited to 5-second intervals
- Metrics are stored in memory and exported on demand

## Troubleshooting

### Common Issues

1. **Metrics endpoint returns 404**: Ensure the server is running and the route is properly configured
2. **No system metrics**: Check that the metrics updater task is running
3. **Stale worker metrics**: Verify that workers are being processed and metrics are being updated in the controller loop

### Debugging

Enable debug logging to see metrics collection:
```bash
RUST_LOG=debug cargo run
```

Check specific metric families:
```bash
curl -s http://localhost:3001/metrics | grep dagda_workers
```
