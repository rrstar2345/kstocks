pub mod nse;
pub mod db;
pub mod settings;

use db::{init_db, start_tick_writer};
use settings::{setup_app_folders};
use nse::OptionStreamer;
use chrono::{Local, Datelike, Duration};

use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

use tracing::{info, error, warn};

type ActiveStreams = Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>;

// Store this in your app state (requires Tauri state management)
// For now, a simpler approach: check if a symbol is already streaming

lazy_static::lazy_static! {
    static ref ACTIVE_STREAMS: ActiveStreams = Arc::new(RwLock::new(HashMap::new()));
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
async fn store_ticks(symbol: String) -> Result<String, String> {
    let mut streams = ACTIVE_STREAMS.write().await;
    
    // Check if already streaming
    if streams.contains_key(&symbol) {
        return Err(format!("Stream already active for {}", symbol));
    }

    let paths = setup_app_folders()
        .map_err(|e| format!("Failed to setup folders: {}", e))?;

    let db_file = paths.db.join("kstocks.db");
    let pool = init_db(&db_file).await
        .map_err(|e| format!("Failed to init database: {}", e))?;

    let (tx, writer_handle) = start_tick_writer(pool);

    tokio::spawn(async move {
        if let Err(e) = writer_handle.await {
            error!("❌ Writer task error: {}", e);
        }
    });

    let valid_symbols = vec!["NIFTY", "NIFTYNXT50", "FINNIFTY", "BANKNIFTY", "MIDCPNIFTY"];
    if !valid_symbols.contains(&symbol.as_str()) {
        warn!("Invalid symbol: {}", symbol);
        return Err(format!("Invalid symbol: {}", symbol));
    }

    let expiry = get_next_thursday_expiry();
    let symbol_clone = symbol.clone();
    // let symbol_for_cleanup = symbol.clone();

    let handle = tokio::spawn(async move {
        let streamer = OptionStreamer::new(symbol_clone.clone(), expiry, tx);
        match streamer.start().await {
            Ok(_) => info!("✅ Stream completed for {}", symbol_clone),
            Err(e) => error!("❌ Stream error for {}: {}", symbol_clone, e),
        }
        
        // Cleanup on completion
        let mut streams = ACTIVE_STREAMS.write().await;
        streams.remove(&symbol_clone);
    });

    streams.insert(symbol.clone(), handle);

    Ok(format!("Data fetch started for {}", symbol))
}


fn get_next_thursday_expiry() -> String {
    let mut date = Local::now().date_naive();
    
    // Move to next day and find the first Thursday
    date = date + Duration::days(1);
    while date.weekday() != chrono::Weekday::Thu {
        date = date + Duration::days(1);
    }
    
    // Format as DD-MMM-YYYY (e.g., "27-Jun-2026")
    date.format("%d-%b-%Y").to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 Starting KSTOCKS application");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![store_ticks])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
