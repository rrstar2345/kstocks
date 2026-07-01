pub mod option_streamer;
pub mod db;
pub mod settings;
pub mod index_tracker;
pub mod indices_streamer;
pub mod index_chart;

use db::{init_db, start_tick_writer};
use settings::{setup_app_folders, load_or_create_config, AppConfig};
use option_streamer::OptionStreamer;
use index_tracker::{get_filtered_symbols, get_all_index_stats_bulk};
use index_chart::{fetch_index_chart, ChartDataPoint};
use indices_streamer::{get_index_cards as get_streamed_index_cards, start_indices_streamer, stop_indices_streamer, seed_index_cache, register_app_handle, IndexCard};
use serde::{Deserialize, Serialize};

use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::sync::OnceLock;

use tracing::{info, error, warn};
use tauri::Manager;

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

/// Fetch the initial index cards via the `indices_stats` endpoint and seed the
/// shared cache used by `get_index_cards`. This is intended to be called once
/// on app load so cards can be drawn immediately, before/independently of the
/// `indices_streamer` websocket connecting.
async fn initialize_index_cards(config: &AppConfig) -> Result<(), String> {
    let symbol_mapping = get_filtered_symbols(config)
        .await
        .map_err(|e| format!("Failed to fetch symbol mapping: {}", e))?;

    let cards = get_all_index_stats_bulk(config)
        .await
        .map_err(|e| format!("Failed to fetch initial index stats: {}", e))?;

    // Re-key cards by fno_symbol to match how the live streamer caches them.
    let mut cards_by_symbol: HashMap<String, IndexCard> = HashMap::new();
    for card in cards {
        if let Some(fno_symbol) = symbol_mapping
            .iter()
            .find(|(_, mapping)| mapping.short_name == card.index_name)
            .map(|(fno_symbol, _)| fno_symbol.clone())
        {
            cards_by_symbol.insert(
                fno_symbol,
                IndexCard {
                    index_name: card.index_name,
                    last_price: card.last_price,
                    change: card.change,
                    change_percent: card.change_percent,
                    is_positive: card.is_positive,
                    dissemination_time: card.dissemination_time,
                },
            );
        }
    }

    seed_index_cache(cards_by_symbol).await;
    info!("✅ Initialized index cards from indices_stats endpoint");
    Ok(())
}

#[tauri::command]
async fn get_index_cards() -> Result<Vec<IndexCard>, String> {
    let config = get_config()?;
    match get_streamed_index_cards(&config).await {
        Ok(cards) => Ok(cards),
        Err(e) => Err(format!("Failed to fetch index cards: {}", e))
    }
}

#[tauri::command]
async fn stop_streamer() -> Result<String, String> {
    stop_indices_streamer().await;
    Ok("Indices streamer stopped".to_string())
}

#[tauri::command]
async fn start_streamer(app_handle: tauri::AppHandle) -> Result<String, String> {
    let config = get_config()?;
    register_app_handle(app_handle).await;
    match start_indices_streamer(&config).await {
        Ok(_) => Ok("Indices streamer started".to_string()),
        Err(e) => Err(format!("Failed to start indices streamer: {}", e))
    }
}

/// Settings the frontend needs at runtime (refresh/poll intervals, time range
/// flags, etc.), sourced entirely from settings.json - never hardcoded in the UI.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "serde")]
pub struct AppSettings {
    pub index_chart_refresh_interval_seconds: u64,
    pub cards_fallback_poll_interval_seconds: u64,
    pub time_range_flags: Vec<String>,
    pub default_time_range_flag: String,
}

#[tauri::command]
async fn get_app_settings() -> Result<AppSettings, String> {
    let config = get_config()?;
    Ok(AppSettings {
        index_chart_refresh_interval_seconds: config.system.index_chart_refresh_interval_seconds,
        cards_fallback_poll_interval_seconds: config.system.cards_fallback_poll_interval_seconds,
        time_range_flags: config.system.time_range_flags.clone(),
        default_time_range_flag: config.user.default_time_range_flag.clone(),
    })
}

#[tauri::command]
async fn get_index_chart_data(
    #[allow(non_snake_case)]
    index_display_name: String,
    #[allow(non_snake_case)]
    time_range_flag: String,
) -> Result<Vec<ChartDataPoint>, String> {
    let config = get_config()?;
    match fetch_index_chart(&index_display_name, &time_range_flag, &config).await {
        Ok(data) => Ok(data),
        Err(e) => Err(format!("Failed to fetch chart data: {}", e)),
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
        .setup(|app| {
            // Start the indices streamer on app launch using tauri::async_runtime
            let app_handle = app.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                tauri::async_runtime::block_on(async {
                    // Register the AppHandle first so the initial seed below (and
                    // the live streamer) can push updates to the frontend.
                    register_app_handle(app_handle).await;

                    match get_config() {
                        Ok(config) => {
                            // Seed the cards from indices_stats first so the UI has
                            // data to draw immediately on load.
                            if let Err(e) = initialize_index_cards(&config).await {
                                eprintln!("❌ Failed to initialize index cards: {}", e);
                            }

                            // indices_streamer is optional: it only refines/updates
                            // the cards live while the market is PO/NM (see
                            // live_update_market_statuses in settings).
                            match start_indices_streamer(&config).await {
                                Ok(_) => info!("✅ Indices streamer started successfully"),
                                Err(e) => eprintln!("❌ Failed to start indices streamer: {}", e),
                            }
                        }
                        Err(e) => eprintln!("Failed to load config: {}", e),
                    }
                })
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            fetch_expiry_dates, 
            store_ticks,
            get_symbol_info,
            get_index_cards,
            start_streamer,
            stop_streamer,
            get_index_chart_data,
            get_app_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}