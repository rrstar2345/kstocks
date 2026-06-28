use anyhow::Result;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::db::{TickRow, TickSender};

use tracing::{info, error, warn};

// ============================================================================
// DATA STRUCTURES - Matching NSE API response format
// ============================================================================

/// Complete option chain message from NSE
#[derive(Debug, Deserialize, Serialize, Clone)]
struct OptionChainMessage {
    #[serde(rename = "expiryDates")]
    expiry_dates: String,
    #[serde(rename = "strikePrice")]
    strike_price: f64,
    timestamp: String,
    #[serde(rename = "PE")]
    pe: Option<OptionData>,
    #[serde(rename = "CE")]
    ce: Option<OptionData>,
    flag: Option<String>,
}

/// Individual option leg data (Call/Put)
#[derive(Debug, Deserialize, Serialize, Clone)]
struct OptionData {
    #[serde(rename = "strikePrice")]
    strike_price: f64,
    #[serde(rename = "expiryDate")]
    expiry_date: String,
    underlying: String,
    identifier: String,
    #[serde(rename = "totalTradedVolume")]
    total_traded_volume: u64,
    #[serde(rename = "lastPrice")]
    last_price: f64,
    change: f64,

    #[serde(rename = "totalBuyQuantity")]
    total_buy_quantity: u64,
    #[serde(rename = "totalSellQuantity")]
    total_sell_quantity: u64,

    #[serde(rename = "buyPrice1")]
    buy_price1: f64,
    #[serde(rename = "buyQuantity1")]
    buy_quantity1: u64,

    #[serde(rename = "sellPrice1")]
    sell_price1: f64,
    #[serde(rename = "sellQuantity1")]
    sell_quantity1: u64,

    #[serde(rename = "optionType")]
    option_type: String,
}


pub struct OptionStreamer {
    symb: String,
    exp: String,
    tx: TickSender,
}

impl OptionStreamer {
    pub fn new(symbol: String, expiry: String, tx: TickSender) -> Self {
        Self { symb: symbol, exp: expiry, tx }
    }

    pub async fn start(&self) -> Result<()> {
        let url = format!(
            "wss://streamer.nseindia.com/streams/fo/mbp?symbol={}&expiry={}",
            self.symb, self.exp
        );

        let (mut ws_stream, _) = connect_async(url).await
            .map_err(|e| anyhow::anyhow!("WebSocket connection failed: {e}"))?;

        while let Some(msg_result) = ws_stream.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    let parsed: OptionChainMessage = match serde_json::from_str(&text) {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("⚠️ JSON parse error: {}", e);
                            continue;
                        }
                    };

                    let row = TickRow {
                        timestamp: parsed.timestamp,
                        symbol: self.symb.clone(),
                        expiry: parsed.expiry_dates,
                        strike_price: parsed.strike_price,

                        ce_last_price: parsed.ce.as_ref().map(|x| x.last_price),
                        ce_change: parsed.ce.as_ref().map(|x| x.change),
                        ce_volume: parsed.ce.as_ref().map(|x| x.total_traded_volume as i64),
                        ce_bid: parsed.ce.as_ref().map(|x| x.buy_price1),
                        ce_ask: parsed.ce.as_ref().map(|x| x.sell_price1),

                        pe_last_price: parsed.pe.as_ref().map(|x| x.last_price),
                        pe_change: parsed.pe.as_ref().map(|x| x.change),
                        pe_volume: parsed.pe.as_ref().map(|x| x.total_traded_volume as i64),
                        pe_bid: parsed.pe.as_ref().map(|x| x.buy_price1),
                        pe_ask: parsed.pe.as_ref().map(|x| x.sell_price1),
                    };

                    if self.tx.send(row).await.is_err() {
                        info!("DB writer channel closed; stopping stream.");
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(e) => {
                    error!("❌ WebSocket error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }
}
