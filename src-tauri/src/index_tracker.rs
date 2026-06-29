use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::settings::AppConfig;

// ============================================================================
// DATA STRUCTURES
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexInfo {
    #[serde(rename = "fnoIndexName")]
    pub fno_index_name: Option<String>,
    #[serde(rename = "indicesLongName")]
    pub indices_long_name: String,
    #[serde(rename = "indicesShortName")]
    pub indices_short_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndicesResponse {
    pub data: HashMap<String, IndexInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexStatData {
    #[serde(rename = "dividentYield")]
    pub divident_yield: f64,
    #[serde(rename = "ffm")]
    pub ffm: f64,
    #[serde(rename = "full")]
    pub full: f64,
    #[serde(rename = "high")]
    pub high: f64,
    #[serde(rename = "icChange")]
    pub ic_change: f64,
    #[serde(rename = "icPerChange")]
    pub ic_per_change: f64,
    #[serde(rename = "indexName")]
    pub index_name: String,
    #[serde(rename = "indicativeClose")]
    pub indicative_close: f64,
    #[serde(rename = "last")]
    pub last: f64,
    #[serde(rename = "low")]
    pub low: f64,
    #[serde(rename = "open")]
    pub open: f64,
    #[serde(rename = "pbRatio")]
    pub pb_ratio: f64,
    #[serde(rename = "peRatio")]
    pub pe_ratio: f64,
    #[serde(rename = "percChange")]
    pub perc_change: f64,
    #[serde(rename = "previousClose")]
    pub previous_close: f64,
    #[serde(rename = "timeVal")]
    pub time_val: String,
    #[serde(rename = "value")]
    pub value: f64,
    #[serde(rename = "volume")]
    pub volume: f64,
    #[serde(rename = "yearHigh")]
    pub year_high: f64,
    #[serde(rename = "yearHighDt")]
    pub year_high_dt: Option<String>,
    #[serde(rename = "yearLow")]
    pub year_low: f64,
    #[serde(rename = "yearLowDt")]
    pub year_low_dt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexStatsResponse {
    pub data: Vec<IndexStatData>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexCard {
    pub index_name: String,
    pub last_price: f64,
    pub change: f64,
    pub change_percent: f64,
    pub is_positive: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolMapping {
    pub fno_symbol: String,
    pub display_name: String,
    pub long_name: String,
    pub short_name: String,
}

// ============================================================================
// CACHED SYMBOL MAPPING (Session-level caching)
// ============================================================================

lazy_static::lazy_static! {
    static ref SYMBOL_CACHE: Arc<RwLock<Option<HashMap<String, SymbolMapping>>>> = 
        Arc::new(RwLock::new(None));
}

/// Build URL from endpoint config with dynamic parameters replaced using config param_key
fn build_url(
    base: &str,
    params: &Option<Vec<crate::settings::ApiParam>>,
    symbol_mapping: &HashMap<String, SymbolMapping>,
) -> String {
    let mut url = base.to_string();

    if let Some(param_list) = params {
        let mut first = true;
        for param in param_list {
            let value = if param.dynamic {
                // Use param_key to determine what value to use
                if let Some(param_key_name) = &param.param_key {
                    get_param_value(param_key_name, symbol_mapping)
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

/// Get parameter value based on param_key name
/// For symbol operations, we use the first symbol in the map
/// For index operations, we use the display name
fn get_param_value(param_key: &str, symbol_mapping: &HashMap<String, SymbolMapping>) -> Option<String> {
    match param_key {
        "fno_symbol" => symbol_mapping.values().next().map(|m| m.fno_symbol.clone()),
        "index_display_name" => symbol_mapping.values().next().map(|m| m.display_name.clone()),
        "time_range_flag" => Some("1M".to_string()), // Default flag, should be parameterized
        "expiry_date" => Some("30-Jun-2026".to_string()), // Default expiry
        _ => None,
    }
}

/// Fetch all indices and filter by valid symbols from config.
/// Caches result in memory for the session.
pub async fn get_filtered_symbols(config: &AppConfig) -> Result<HashMap<String, SymbolMapping>> {
    // Check cache first
    {
        let cache = SYMBOL_CACHE.read().await;
        if let Some(ref cached) = *cache {
            info!("📦 Returning cached symbol mapping");
            return Ok(cached.clone());
        }
    }

    let url = build_url(&config.system.indices_info.base, &config.system.indices_info.params, &HashMap::new());
    let client = reqwest::Client::new();

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .map_err(|e| anyhow!("Failed to fetch indices info: {}", e))?;

    let data: IndicesResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse indices response: {}", e))?;

    // Filter symbols based on valid_symbols from config
    let mut filtered = HashMap::new();
    for (index_display_name, info) in data.data {
        if let Some(ref fno_name) = info.fno_index_name {
            if config.user.valid_symbols.contains(fno_name) {
                filtered.insert(
                    fno_name.clone(),
                    SymbolMapping {
                        fno_symbol: fno_name.clone(),
                        display_name: index_display_name.clone(),
                        long_name: info.indices_long_name.clone(),
                        short_name: info.indices_short_name.clone(),
                    },
                );
                info!("✅ Added symbol: {} -> {} ({})", fno_name, index_display_name, info.indices_long_name);
            }
        }
    }

    // Cache the result
    {
        let mut cache = SYMBOL_CACHE.write().await;
        *cache = Some(filtered.clone());
    }

    info!("🎯 Fetched and cached {} valid symbols", filtered.len());
    Ok(filtered)
}

/// Fetch high-level stats for a single index symbol.
pub async fn get_index_stats(fno_symbol: &str, config: &AppConfig) -> Result<IndexCard> {
    // Get the symbol mapping to find display name
    let symbol_mapping = get_filtered_symbols(config).await?;

    let mapping = symbol_mapping
        .get(fno_symbol)
        .ok_or_else(|| anyhow!("Symbol {} not found in mapping", fno_symbol))?;

    // Build URL with the display name from mapping
    let mut param_mapping = HashMap::new();
    param_mapping.insert(fno_symbol.to_string(), mapping.clone());

    let url = build_url(
        &config.system.index_info.base,
        &config.system.index_info.params,
        &param_mapping,
    );

    let client = reqwest::Client::new();

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .map_err(|e| anyhow!("Failed to fetch index stats: {}", e))?;

    let data: IndexStatsResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse index stats response: {}", e))?;

    if data.data.is_empty() {
        return Err(anyhow!("No data returned for symbol: {}", fno_symbol));
    }

    let stat = &data.data[0];
    let card = IndexCard {
        index_name: stat.index_name.clone(),
        last_price: stat.last,
        change: stat.last - stat.previous_close,
        change_percent: stat.perc_change,
        is_positive: stat.perc_change >= 0.0,
    };

    info!("📊 Fetched stats for {}: {} ({}%)", 
        fno_symbol, card.last_price, card.change_percent);

    Ok(card)
}

/// Fetch stats for all valid symbols in config.
pub async fn get_all_index_stats(config: &AppConfig) -> Result<Vec<IndexCard>> {
    let symbols = config.user.valid_symbols.clone();
    let mut cards = Vec::new();

    for symbol in symbols {
        match get_index_stats(&symbol, config).await {
            Ok(card) => cards.push(card),
            Err(e) => warn!("⚠️ Failed to fetch stats for {}: {}", symbol, e),
        }
    }

    Ok(cards)
}

/// Clear the cached symbol mapping (useful for manual refresh).
pub async fn clear_symbol_cache() {
    let mut cache = SYMBOL_CACHE.write().await;
    *cache = None;
    info!("🗑️ Cleared symbol cache");
}