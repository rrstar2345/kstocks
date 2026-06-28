pub mod option_streamer;
pub mod db;
pub mod settings;
pub mod index_tracker;

use db::{init_db, start_tick_writer};
use settings::{setup_app_folders, load_or_create_config, AppConfig};
use option_streamer::OptionStreamer;
use index_tracker::{get_filtered_symbols, get_all_index_stats, IndexCard};
use serde::{Deserialize, Serialize};

use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::sync::OnceLock;

use tracing::{info, error, warn};

type ActiveStreams = Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "serde")]
pub struct ExpiryResponse {
    #[serde(rename = "expiryDates")]
    pub expiry_dates: Vec<String>,
    #[serde(rename = "strikePrice")]
    pub strike_price: Vec<String>,
}

lazy_static::lazy_static! {
    static ref ACTIVE_STREAMS: ActiveStreams = Arc::new(RwLock::new(HashMap::new()));
}

static APP_CONFIG: OnceLock<AppConfig> = OnceLock::new();

fn get_config() -> Result<AppConfig, String> {
    if let Some(config) = APP_CONFIG.get() {
        return Ok(config.clone());
    }

    let paths = setup_app_folders()
        .map_err(|e| format!("Failed to setup folders: {}", e))?;
    
    let config = load_or_create_config(&paths.settings_file)
        .map_err(|e| format!("Failed to load config: {}", e))?;
    
    APP_CONFIG.set(config.clone()).ok();
    Ok(config)
}

/// Build URL from endpoint config with dynamic parameters replaced using config param_key
fn build_url_from_config(
    base: &str,
    params: &Option<Vec<settings::ApiParam>>,
    runtime_params: &HashMap<String, String>,
) -> String {
    let mut url = base.to_string();

    if let Some(param_list) = params {
        let mut first = true;
        for param in param_list {
            let value = if param.dynamic {
                // Use param_key to determine what value to use
                if let Some(param_key_name) = &param.param_key {
                    runtime_params
                        .get(param_key_name)
                        .cloned()
                        .unwrap_or_else(|| param.value.clone())
                } else {
                    param.value.clone()
                }
            } else {
                param.value.clone()
            };

            url.push_str(if first { "?" } else { "&" });
            url.push_str(&format!("{}={}", param.key, urlencoding::encode(&value)));
            first = false;
        }
    }

    url
}

#[tauri::command]
async fn fetch_expiry_dates(symbol: String) -> Result<serde_json::Value, String> {
    let config = get_config()?;

    let mut runtime_params = HashMap::new();
    runtime_params.insert("fno_symbol".to_string(), symbol.clone());

    let url = build_url_from_config(
        &config.system.option_info.base,
        &config.system.option_info.params,
        &runtime_params
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch expiry dates: {}", e))?;

    let data: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse expiry dates: {}", e))?;

    let result = serde_json::json!({
        "expiry_dates": data.get("expiryDates").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
        "strike_price": data.get("strikePrice").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
    });

    Ok(result)
}

#[tauri::command]
async fn store_ticks(symbol: String, expiry_date: String) -> Result<String, String> {
    info!("store_ticks called with symbol: {}, expiry_date: {}", symbol, expiry_date);

    let config = get_config()?;

    let mut streams = ACTIVE_STREAMS.write().await;
    
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

    if !config.user.valid_symbols.contains(&symbol) {
        warn!("Invalid symbol: {}", symbol);
        return Err(format!("Invalid symbol: {}", symbol));
    }

    let symbol_clone = symbol.clone();

    let handle = tokio::spawn(async move {
        let streamer = OptionStreamer::new(symbol_clone.clone(), expiry_date, tx);
        match streamer.start().await {
            Ok(_) => info!("✅ Stream completed for {}", symbol_clone),
            Err(e) => error!("❌ Stream error for {}: {}", symbol_clone, e),
        }
        
        let mut streams = ACTIVE_STREAMS.write().await;
        streams.remove(&symbol_clone);
    });

    streams.insert(symbol.clone(), handle);

    Ok(format!("Data fetch started for {}", symbol))
}

#[tauri::command]
async fn get_symbol_info() -> Result<HashMap<String, serde_json::Value>, String> {
    let config = get_config()?;

    match get_filtered_symbols(&config).await {
        Ok(symbols) => {
            let result: HashMap<String, serde_json::Value> = symbols
                .into_iter()
                .map(|(k, v)| (k, serde_json::to_value(v).unwrap_or(serde_json::Value::Null)))
                .collect();
            Ok(result)
        }
        Err(e) => Err(format!("Failed to fetch symbol info: {}", e))
    }
}

#[tauri::command]
async fn get_index_cards() -> Result<Vec<IndexCard>, String> {
    let config = get_config()?;

    match get_all_index_stats(&config).await {
        Ok(cards) => Ok(cards),
        Err(e) => Err(format!("Failed to fetch index stats: {}", e))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 Starting KSTOCKS application");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            fetch_expiry_dates, 
            store_ticks,
            get_symbol_info,
            get_index_cards,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}