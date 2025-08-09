use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;
use rand::Rng;
use clap::Parser;

/// Load test configuration
#[derive(Parser)]
#[command(name = "load_test")]
#[command(about = "A load test for the Ticker PoC application")]
struct Args {
    /// Number of times to run the test sequence
    #[arg(short, long, default_value_t = 10)]
    iterations: usize,
    
    /// Number of inventory IDs to create and test
    #[arg(short = 'n', long, default_value_t = 5)]
    inventories: usize,
    
    /// Base URL for the server
    #[arg(short, long, default_value = "http://localhost:3001")]
    base_url: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    println!("🚀 Starting Ticker PoC Load Test");
    println!("Iterations: {}", args.iterations);
    println!("Inventory IDs: {}", args.inventories);
    println!("Base URL: {}", args.base_url);
    println!();

    let client = Client::new();
    
    // Generate random UUIDs for inventory IDs
    let inventory_ids: Vec<Uuid> = (0..args.inventories)
        .map(|_| Uuid::new_v4())
        .collect();
    
    println!("📋 Generated Inventory IDs:");
    for (i, id) in inventory_ids.iter().enumerate() {
        println!("  {}. {}", i + 1, id);
    }
    println!();

    // Run the sequence n times
    for iteration in 1..=args.iterations {
        println!("🔄 Iteration {}/{}", iteration, args.iterations);
        
        for (idx, inventory_id) in inventory_ids.iter().enumerate() {
            println!("  📦 Processing Inventory {} ({})", idx + 1, inventory_id);
            
            // Step 1: POST request to create a house
            let create_response = create_house(&client, *inventory_id, &args.base_url).await?;
            let worker_id = extract_worker_id(&create_response)?;
            println!("    ✅ Created house worker: {}", worker_id);
            
            // Step 2: PUT request to start the worker
            start_worker(&client, *inventory_id, worker_id, &args.base_url).await?;
            println!("    ▶️  Started worker");
            
            // Step 3: Random action based on number 0-4
            let random_num = rand::thread_rng().gen_range(0..=4);
            println!("    🎲 Random number: {}", random_num);
            
            match random_num {
                3 => {
                    // Pause, then resume after 500ms
                    pause_worker(&client, *inventory_id, worker_id, &args.base_url).await?;
                    println!("    ⏸️  Paused worker");
                    
                    sleep(Duration::from_millis(500)).await;
                    
                    resume_worker(&client, *inventory_id, worker_id, &args.base_url).await?;
                    println!("    ▶️  Resumed worker after 500ms");
                }
                4 => {
                    // Just pause
                    pause_worker(&client, *inventory_id, worker_id, &args.base_url).await?;
                    println!("    ⏸️  Paused worker");
                }
                _ => {
                    // No action for 0, 1, 2
                    println!("    ➡️  No additional action");
                }
            }
            
            // Small delay between workers
            sleep(Duration::from_millis(100)).await;
        }
        
        // Delay between iterations
        if iteration < args.iterations {
            sleep(Duration::from_millis(500)).await;
        }
        println!();
    }

    println!("📊 Final Status Check");
    println!("===================");
    
    // Check inventory status for each ID
    for (idx, inventory_id) in inventory_ids.iter().enumerate() {
        println!("📦 Inventory {} Status ({})", idx + 1, inventory_id);
        let status = get_inventory_status(&client, *inventory_id, &args.base_url).await?;
        print_inventory_status(&status);
        println!();
    }
    
    // Get system status
    println!("🖥️  System Status");
    let system_status = get_system_status(&client, &args.base_url).await?;
    print_system_status(&system_status);
    println!();
    
    // Get metrics sample
    println!("📈 Metrics Sample");
    let metrics = get_metrics_sample(&client, &args.base_url).await?;
    print_metrics_sample(&metrics);
    
    println!("✨ Load test completed successfully!");
    Ok(())
}

async fn create_house(client: &Client, inventory_id: Uuid, base_url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let response = client
        .post(&format!("{}/inventory/house", base_url))
        .header("Authorization", format!("Bearer {}", inventory_id))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Create house failed: {}", response.status()).into());
    }
    
    Ok(response.json().await?)
}

async fn start_worker(client: &Client, inventory_id: Uuid, worker_id: Uuid, base_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = client
        .put(&format!("{}/inventory/{}/start", base_url, worker_id))
        .header("Authorization", format!("Bearer {}", inventory_id))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Start worker failed: {}", response.status()).into());
    }
    
    Ok(())
}

async fn pause_worker(client: &Client, inventory_id: Uuid, worker_id: Uuid, base_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = client
        .put(&format!("{}/inventory/{}/pause", base_url, worker_id))
        .header("Authorization", format!("Bearer {}", inventory_id))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Pause worker failed: {}", response.status()).into());
    }
    
    Ok(())
}

async fn resume_worker(client: &Client, inventory_id: Uuid, worker_id: Uuid, base_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = client
        .put(&format!("{}/inventory/{}/resume", base_url, worker_id))
        .header("Authorization", format!("Bearer {}", inventory_id))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Resume worker failed: {}", response.status()).into());
    }
    
    Ok(())
}

async fn get_inventory_status(client: &Client, inventory_id: Uuid, base_url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let response = client
        .get(&format!("{}/inventory/status", base_url))
        .header("Authorization", format!("Bearer {}", inventory_id))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Get inventory status failed: {}", response.status()).into());
    }
    
    Ok(response.json().await?)
}

async fn get_system_status(client: &Client, base_url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let response = client
        .get(&format!("{}/system/status", base_url))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Get system status failed: {}", response.status()).into());
    }
    
    Ok(response.json().await?)
}

async fn get_metrics_sample(client: &Client, base_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let response = client
        .get(&format!("{}/metrics", base_url))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Get metrics failed: {}", response.status()).into());
    }
    
    Ok(response.text().await?)
}

fn extract_worker_id(response: &Value) -> Result<Uuid, Box<dyn std::error::Error>> {
    let worker_id_str = response["worker_id"]
        .as_str()
        .ok_or("No worker_id in response")?;
    Ok(Uuid::parse_str(worker_id_str)?)
}

fn print_inventory_status(status: &Value) {
    if let Some(workers) = status.as_array() {
        if workers.is_empty() {
            println!("  No workers found");
            return;
        }
        
        for worker in workers {
            let id = worker["id"].as_str().unwrap_or("unknown");
            let name = worker["name"].as_str().unwrap_or("unknown");
            let status = worker["status"].as_str().unwrap_or("unknown");
            let progress = &worker["progress"];
            let current = progress["current_ticks"].as_u64().unwrap_or(0);
            let total = progress["total_ticks"].as_u64().unwrap_or(0);
            let percentage = progress["percentage"].as_f64().unwrap_or(0.0);
            
            println!("  Worker: {} ({})", &id[..8], name);
            println!("    Status: {}", status);
            println!("    Progress: {}/{} ({:.1}%)", current, total, percentage);
        }
    }
}

fn print_system_status(status: &Value) {
    let system_status = status["status"].as_str().unwrap_or("unknown");
    let shards = status["shards"].as_u64().unwrap_or(0);
    
    println!("  Status: {}", system_status);
    println!("  Shards: {}", shards);
}

fn print_metrics_sample(metrics: &str) {
    println!("  Key metrics:");
    
    for line in metrics.lines() {
        if line.starts_with("dagda_status ") {
            println!("    {}", line);
        } else if line.starts_with("dagda_shards_total ") {
            println!("    {}", line);
        } else if line.starts_with("dagda_workers_total") && !line.contains("bucket") {
            println!("    {}", line);
        } else if line.starts_with("http_requests_total") && !line.contains("bucket") {
            println!("    {}", line);
        } else if line.starts_with("system_cpu_usage_percent{cpu=\"total\"}") {
            println!("    {}", line);
        } else if line.starts_with("system_memory_usage_bytes{type=\"used\"}") {
            println!("    {}", line);
        }
    }
}
