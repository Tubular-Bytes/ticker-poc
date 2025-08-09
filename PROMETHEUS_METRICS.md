# Ticker PoC - Prometheus Metrics Documentation

## Overview

The Ticker PoC application now includes comprehensive Prometheus metrics for monitoring system performance, worker lifecycles, and HTTP request patterns.

## Available Metrics

### System Metrics
- `system_cpu_usage_percent{cpu="[cpu_id|total]"}` - CPU usage percentage per core and total
- `system_memory_total_bytes{type="total"}` - Total system memory
- `system_memory_usage_bytes{type="[used|available]"}` - Memory usage details

### Dagda Controller Metrics
- `dagda_status` - Controller status (0=stopped, 1=running, 2=stopping)
- `dagda_shards_total` - Total number of active shards
- `dagda_workers_total{shard_id="<shard_uuid>"}` - Workers per shard
- `dagda_message_queue_size{queue_type="dagda_inbox"}` - Message queue backpressure

### Worker Operation Metrics
- `dagda_worker_operations_total{operation="<op>",status="<status>"}` - Worker operations
  - Operations: `task_with_parent_created`, `worker_start_with_parent`, `worker_pause_with_parent`, `worker_resume_with_parent`, `completed`
  - Status: `success`, `error`

### Performance Metrics
- `dagda_tick_processing_duration_seconds` - Histogram of tick processing times
- `dagda_worker_processing_duration_seconds{message_type="<type>"}` - Worker message processing times

### HTTP Request Metrics
- `http_requests_total{endpoint="<endpoint>",method="<method>",status="<code>"}` - HTTP request counters
- `http_request_duration_seconds{endpoint="<endpoint>",method="<method>"}` - Request duration histogram

## Accessing Metrics

Metrics are available at: `http://localhost:3001/metrics`

```bash
# View all metrics
curl http://localhost:3001/metrics

# Check worker counts per shard
curl -s http://localhost:3001/metrics | grep dagda_workers_total

# Monitor HTTP traffic
curl -s http://localhost:3001/metrics | grep http_requests_total

# System resource monitoring
curl -s http://localhost:3001/metrics | grep -E "(system_|memory_)"
```

## Load Testing

Run the included load test to generate realistic metrics:

```bash
cargo run --example load_test
```

The load test:
- Creates 5 random inventory IDs
- Runs 10 iterations (configurable via `NUM_ITERATIONS`)
- Creates house workers for each inventory
- Randomly pauses/resumes workers based on random numbers 0-4
- Provides comprehensive status reporting

## Sample Metrics After Load Test

After running the load test, you'll see metrics like:

```
dagda_shards_total 6
dagda_workers_total{shard_id="1d71fb4a-3f2f-441d-8c09-b6b265bf1800"} 10
dagda_workers_total{shard_id="563619d0-cf18-461b-a4ae-0afc634e543d"} 10

http_requests_total{endpoint="/inventory/{name}",method="POST",status="200"} 52
http_requests_total{endpoint="/inventory/{worker_id}/start",method="PUT",status="200"} 50
http_requests_total{endpoint="/inventory/{worker_id}/pause",method="PUT",status="200"} 26
http_requests_total{endpoint="/inventory/{worker_id}/resume",method="PUT",status="200"} 14

system_cpu_usage_percent{cpu="total"} 20.578393936157227
system_memory_usage_bytes{type="used"} 13735755776
```

## Prometheus Configuration

To scrape these metrics with Prometheus, add this job to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'ticker-poc'
    static_configs:
      - targets: ['localhost:3001']
    metrics_path: '/metrics'
    scrape_interval: 15s
```

## Key Features

1. **Real-time Monitoring**: All metrics update in real-time as the application runs
2. **Worker Lifecycle Tracking**: Complete visibility into worker creation, state changes, and completion
3. **Performance Monitoring**: Detailed timing information for all operations
4. **Resource Monitoring**: System CPU and memory usage tracking
5. **HTTP Request Monitoring**: Complete request patterns and response times
6. **Backpressure Monitoring**: Message queue sizes to detect system stress

## Architecture

- **Global Registry**: Thread-safe OnceLock pattern for metrics access
- **Middleware Integration**: Automatic HTTP request metrics collection
- **Background Monitoring**: System resource monitoring in separate task
- **Comprehensive Coverage**: Metrics instrumentation throughout the Dagda controller

The metrics implementation provides complete observability for production deployment and performance analysis.
