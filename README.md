# Ticker POC

Quick proof of concept of a ticker-based actor system serving as a game backend.

Can start, pause, resume and cancel running operations. Once the required ticks is fulfilled the building is considered completed.

The plan is to extend this proof of concept with further lifecycle events and create a working simulation where buildings are built, people are filling jobs and items and materials are refined and produced.

## Running the Application

```bash
RUST_LOG=debug cargo run
```

## Example Output

```
2025-08-04T10:13:24.311964Z  INFO ticker_poc: Starting ticker-poc application
2025-08-04T10:13:25.314512Z  INFO ticker_poc: Sent test task
2025-08-04T10:13:25.314558Z  INFO ticker_poc::dagda: Received task: Blueprint { id: "test-building-1", ticks: 5 }
2025-08-04T10:13:26.313856Z  INFO ticker_poc::dagda: Building started for blueprint: test-building-1
2025-08-04T10:13:26.313895Z DEBUG ticker_poc::dagda: Building progress: 1
2025-08-04T10:13:30.313756Z  INFO ticker_poc::dagda: Building completed for blueprint: test-building-1
```
