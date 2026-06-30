use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

use crate::settings::AppConfig;

// ============================================================================
// DATA STRUCTURES FOR CHART
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChartDataPoint {
    pub timestamp: i64,
    pub price: f64,
    pub status: String,      // "PO" (Pre-open) or "NM" (New Market)
    pub change: f64,         // only for 1D data
    pub change_percent: f64, // only for 1D data
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexChartResponse {
    pub data: ChartData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChartData {
    #[serde(rename = "closePrice")]
    pub close_price: f64,
    #[serde(rename = "grapthData")]
    #[serde(default)]
    pub graph_data_typo: Vec<Vec<serde_json::Value>>,
    #[serde(rename = "graphData")]
    #[serde(default)]
    pub graph_data_correct: Vec<Vec<serde_json::Value>>,
    pub identifier: String,
    pub name: String,
}

impl ChartData {
    pub fn get_graph_data(&self) -> &Vec<Vec<serde_json::Value>> {
        if !self.graph_data_typo.is_empty() {
            &self.graph_data_typo
        } else {
            &self.graph_data_correct
        }
    }
}

// Parse raw chart data into ChartDataPoint objects
pub fn parse_chart_data(raw_data: &[Vec<serde_json::Value>]) -> Result<Vec<ChartDataPoint>> {
    let mut points = Vec::new();

    for item in raw_data {
        if item.len() < 2 {
            continue; // Skip malformed data
        }

        let timestamp = item[0]
            .as_i64()
            .ok_or_else(|| anyhow!("Invalid timestamp"))?;
        
        let price = item[1]
            .as_f64()
            .ok_or_else(|| anyhow!("Invalid price"))?;
        
        let status = item
            .get(2)
            .and_then(|v| v.as_str())
            .unwrap_or("NM")
            .to_string();

        let change = item
            .get(3)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let change_percent = item
            .get(4)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        points.push(ChartDataPoint {
            timestamp,
            price,
            status,
            change,
            change_percent,
        });
    }

    Ok(points)
}

// ============================================================================
// URL BUILDING
// ============================================================================

pub fn build_chart_url(
    base: &str,
    params: &Option<Vec<crate::settings::ApiParam>>,
    runtime_params: &HashMap<String, String>,
) -> String {
    let mut url = base.to_string();

    if let Some(param_list) = params {
        let mut first = true;
        for param in param_list {
            let value = if param.dynamic {
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

// ============================================================================
// API FETCHING
// ============================================================================

/// Fetch chart data for a specific index and time range flag
pub async fn fetch_index_chart(
    index_display_name: &str,
    time_range_flag: &str,
    config: &AppConfig,
) -> Result<Vec<ChartDataPoint>> {
    let client = reqwest::Client::new();
    let mut runtime_params = HashMap::new();
    runtime_params.insert("index_display_name".to_string(), index_display_name.to_string());
    runtime_params.insert("time_range_flag".to_string(), time_range_flag.to_string());

    // Determine which endpoint to use
    let (base, params) = if time_range_flag == "1D" {
        (
            &config.system.index_chart.base,
            &config.system.index_chart.params,
        )
    } else {
        (
            &config.system.historic_index_chart.base,
            &config.system.historic_index_chart.params,
        )
    };

    let url = build_chart_url(base, params, &runtime_params);

    info!("📊 Fetching chart data from: {}", url);

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .map_err(|e| anyhow!("Failed to fetch chart data: {}", e))?;

    let response_text = response
        .text()
        .await
        .map_err(|e| anyhow!("Failed to read response body: {}", e))?;

    info!("📦 Raw response: {}", response_text);

    let data: IndexChartResponse = serde_json::from_str(&response_text)
        .map_err(|e| anyhow!("Failed to parse chart response: {} | Response: {}", e, response_text))?;

    let raw_data = data.data.get_graph_data();
    parse_chart_data(raw_data)
}