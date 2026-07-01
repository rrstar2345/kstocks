use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use futures::stream::StreamExt;
use tracing::{info, warn, error};
use tauri::{AppHandle, Emitter};

use crate::settings::AppConfig;
use crate::index_tracker::get_filtered_symbols;

/// Tauri event name emitted whenever a single index card is created/updated
/// in the cache (from the WebSocket stream or the initial seed).
pub const INDEX_CARD_UPDATE_EVENT: &str = "index-card-update";

// ============================================================================
// DATA STRUCTURES
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexStreamMessage {
    #[serde(rename = "indexName")]
    pub index_name: String,
    #[serde(rename = "currentPrice")]
    pub current_price: f64,
    #[serde(rename = "perChange")]
    pub per_change: f64,
    #[serde(rename = "change")]
    pub change: f64,
    #[serde(rename = "previousClose")]
    pub previous_close: f64,
    #[serde(rename = "dessiminationTime")]
    pub dissemination_time: String,
    pub open: f64,
    pub low: f64,
    pub high: f64,
    #[serde(rename = "indStatus")]
    pub ind_status: String,
    #[serde(rename = "mktStatus")]
    pub mkt_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexCard {
    pub index_name: String,
    pub last_price: f64,
    pub change: f64,
    pub change_percent: f64,
    pub is_positive: bool,
    pub dissemination_time: String,
}

// ============================================================================
// CACHED INDEX DATA (Real-time updates)
// ============================================================================

lazy_static::lazy_static! {
    static ref INDEX_CACHE: Arc<RwLock<HashMap<String, IndexCard>>> = 
        Arc::new(RwLock::new(HashMap::new()));
    
    static ref STREAMER_HANDLE: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>> = 
        Arc::new(RwLock::new(None));
    
    static ref SYMBOL_MAPPING: Arc<RwLock<Option<HashMap<String, String>>>> = 
        Arc::new(RwLock::new(None));

    static ref APP_HANDLE: Arc<RwLock<Option<AppHandle>>> =
        Arc::new(RwLock::new(None));
}

/// Emit the given card to the frontend via a Tauri event, if an AppHandle has
/// been registered (via `start_indices_streamer` or `seed_index_cache`).
async fn emit_card_update(card: &IndexCard) {
    let handle_lock = APP_HANDLE.read().await;
    if let Some(handle) = handle_lock.as_ref() {
        if let Err(e) = handle.emit(INDEX_CARD_UPDATE_EVENT, card) {
            warn!("⚠️ Failed to emit {} event: {}", INDEX_CARD_UPDATE_EVENT, e);
        }
    }
}

/// Build a mapping from streaming index names to internal symbol names using indices_info API
async fn build_symbol_mapping(config: &AppConfig) -> Result<HashMap<String, String>> {
    let symbol_mapping = get_filtered_symbols(config).await?;
    let mut streaming_to_symbol = HashMap::new();
    
    // Build mapping from short names (which match WebSocket stream) to symbols
    for (symbol, mapping) in symbol_mapping.iter() {
        // Use short_name which matches what the WebSocket stream sends
        streaming_to_symbol.insert(mapping.short_name.clone(), symbol.clone());
        
        info!("📍 Mapped streaming name '{}' to symbol '{}'", mapping.short_name, symbol);
    }
    
    Ok(streaming_to_symbol)
}

/// Register the AppHandle used to emit real-time card update events to the
/// frontend. Should be called as early as possible on app startup, before
/// `seed_index_cache`/`start_indices_streamer`, so no updates are missed.
pub async fn register_app_handle(app_handle: AppHandle) {
    let mut app_handle_lock = APP_HANDLE.write().await;
    *app_handle_lock = Some(app_handle);
}

/// Start the WebSocket streamer for indices. Spawns a background task.
/// Requires `register_app_handle` to have been called first so card updates
/// can be pushed to the frontend, removing the need for the UI to poll on a
/// fast interval.
pub async fn start_indices_streamer(config: &AppConfig) -> Result<()> {
    let mut handle_lock = STREAMER_HANDLE.write().await;
    
    if handle_lock.is_some() {
        warn!("⚠️ Indices streamer is already running");
        return Ok(());
    }

    // Build the mapping from indices_info API
    match build_symbol_mapping(config).await {
        Ok(mapping) => {
            let mut symbol_cache = SYMBOL_MAPPING.write().await;
            *symbol_cache = Some(mapping.clone());
            info!("✅ Built symbol mapping with {} entries", mapping.len());
        }
        Err(e) => {
            error!("⚠️ Failed to build symbol mapping, will retry on stream start: {}", e);
        }
    }

    let url = config.system.indices_streamer.base.clone();
    let config_clone = config.clone();

    let handle = tokio::spawn(async move {
        loop {
            match stream_indices(&url, &config_clone).await {
                Ok(_) => {
                    info!("✅ Indices stream completed gracefully");
                    break;
                }
                Err(e) => {
                    error!("❌ Indices stream error: {}. Retrying in 5 seconds...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    *handle_lock = Some(handle);
    info!("🚀 Started indices WebSocket streamer");
    Ok(())
}

/// Stop the WebSocket streamer
pub async fn stop_indices_streamer() {
    let mut handle_lock = STREAMER_HANDLE.write().await;
    if let Some(handle) = handle_lock.take() {
        handle.abort();
        info!("⛔ Stopped indices WebSocket streamer");
    }
}

/// Internal function to handle the WebSocket stream
async fn stream_indices(url: &str, config: &AppConfig) -> Result<()> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| anyhow!("Failed to connect to WebSocket: {}", e))?;

    info!("🔗 Connected to indices WebSocket stream");

    let (_, mut read) = ws_stream.split();

    while let Some(msg_result) = read.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<IndexStreamMessage>(&text) {
                    Ok(stream_msg) => {
                        // Try to get symbol mapping, rebuild if needed
                        let symbol_map = SYMBOL_MAPPING.read().await.clone();
                        let mapping_to_use = if symbol_map.is_some() {
                            symbol_map
                        } else {
                            // Rebuild mapping if not available
                            if let Ok(mapping) = build_symbol_mapping(config).await {
                                let mut symbol_cache = SYMBOL_MAPPING.write().await;
                                *symbol_cache = Some(mapping.clone());
                                Some(mapping)
                            } else {
                                None
                            }
                        };
                        
                        // Only apply live streamer updates while the market is in
                        // pre-open ("PO") or normal-market ("NM") state. Outside of
                        // these states, the initial `indices_stats` snapshot remains
                        // the source of truth for the cards.
                        if let Some(ref mapping) = mapping_to_use {
                            // Look up the FNO symbol from streaming index name
                            if let Some(fno_symbol) = mapping.get(&stream_msg.index_name) {
                                // Check if this symbol is in valid_symbols
                                if config.user.valid_symbols.contains(fno_symbol) {
                                    let card = IndexCard {
                                        index_name: stream_msg.index_name.clone(),
                                        last_price: stream_msg.current_price,
                                        change: stream_msg.change,
                                        change_percent: stream_msg.per_change,
                                        is_positive: stream_msg.per_change >= 0.0,
                                        dissemination_time: stream_msg.dissemination_time.clone(),
                                    };

                                    {
                                        let mut cache = INDEX_CACHE.write().await;
                                        cache.insert(fno_symbol.clone(), card.clone());
                                    }

                                    // Push the update to the frontend immediately instead of
                                    // relying on the UI to poll for it.
                                    emit_card_update(&card).await;

                                    // info!("📊 Updated {}: {} ({}%)", 
                                    //     stream_msg.index_name, stream_msg.current_price, stream_msg.per_change);
                                }
                            } else {
                                // warn!("⚠️ Streaming index name not in mapping: {}", stream_msg.index_name);
                            }
                        } else {
                            warn!("⚠️ Symbol mapping not available for: {}", stream_msg.index_name);
                        }
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to parse stream message: {}. Message: {}", e, text);
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("🔌 WebSocket closed by server");
                break;
            }
            Ok(_) => {
                // Ignore other message types (Ping, Pong, Binary)
            }
            Err(e) => {
                error!("❌ WebSocket error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

/// Seed/replace the index cache with initial data, e.g. from the `indices_stats`
/// bulk call made on app load. `cards` is keyed by fno_symbol (matching how the
/// live streamer keys its cache) mapped to the corresponding IndexCard.
pub async fn seed_index_cache(cards: HashMap<String, IndexCard>) {
    {
        let mut cache = INDEX_CACHE.write().await;
        for (fno_symbol, card) in cards.iter() {
            cache.insert(fno_symbol.clone(), card.clone());
        }
    }

    for card in cards.values() {
        emit_card_update(card).await;
    }

    info!("🌱 Seeded index cache with initial stats");
}

/// Get all currently cached index cards in the order of valid_symbols
pub async fn get_index_cards(config: &AppConfig) -> Result<Vec<IndexCard>> {
    let cache = INDEX_CACHE.read().await;
    
    // Return cards in the same order as valid_symbols
    let mut ordered_cards = Vec::new();
    for symbol in &config.user.valid_symbols {
        if let Some(card) = cache.get(symbol) {
            ordered_cards.push(card.clone());
        }
    }
    
    if ordered_cards.is_empty() {
        warn!("⚠️ No cached index data available yet");
    }
    
    Ok(ordered_cards)
}

/// Check if streamer is currently running
pub async fn is_streamer_running() -> bool {
    STREAMER_HANDLE.read().await.is_some()
}

/// Clear the cached index data (useful for debugging)
pub async fn clear_index_cache() {
    let mut cache = INDEX_CACHE.write().await;
    cache.clear();
    info!("🗑️ Cleared index cache");
}