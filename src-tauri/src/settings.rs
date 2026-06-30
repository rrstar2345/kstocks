use std::fs;
use std::io;
use std::path::{PathBuf};
use std::env;
use serde::{Deserialize, Serialize};
use tracing::{info, error};

pub struct AppPaths {
    pub root: PathBuf,
    pub models: PathBuf,
    pub stt: PathBuf,
    pub llm: PathBuf,
    pub tts: PathBuf,
    pub db: PathBuf,
    pub chats_dir: PathBuf,
    pub settings_file: PathBuf,
    pub model_names_file: PathBuf,
}

pub const APP_NAME: &str = "kstocks";

// ============================================================================
// CONFIGURATION STRUCTURES
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiParam {
    pub key: String,          // actual parameter name in URL (e.g., "index")
    pub value: String,        // default/static value or placeholder name
    pub dynamic: bool,        // is this parameter dynamic (needs runtime replacement)
    pub param_key: Option<String>, // if dynamic, which config key to use (e.g., "indices_short_name")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiEndpoint {
    pub base: String,
    pub params: Option<Vec<ApiParam>>,
    pub desc: String,
    pub wss: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemConfig {
    pub time_range_flags: Vec<String>,
    pub index_chart_refresh_interval_seconds: u64,
    pub market_status_url: ApiEndpoint,
    pub option_info: ApiEndpoint,
    pub option_ticks: ApiEndpoint,
    pub indices_info: ApiEndpoint,
    pub index_info: ApiEndpoint,
    pub index_chart: ApiEndpoint,
    pub historic_index_chart: ApiEndpoint,
    pub indices_streamer: ApiEndpoint,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserConfig {
    pub valid_symbols: Vec<String>,
    pub default_symbol: String,
    pub default_time_range_flag: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub system: SystemConfig,
    pub user: UserConfig,
}

impl AppConfig {
    pub fn default() -> Self {
        AppConfig {
            system: SystemConfig {
                time_range_flags: vec![
                    "1D".to_string(), "1M".to_string(), "3M".to_string(), "6M".to_string(),
                    "1Y".to_string(), "5Y".to_string(), "10Y".to_string(), "15Y".to_string(),
                    "20Y".to_string(), "25Y".to_string(), "30Y".to_string(),
                ],
                index_chart_refresh_interval_seconds: 15,
                market_status_url: ApiEndpoint {
                    base: "https://www.nseindia.com/api/marketStatus".to_string(),
                    params: None,
                    desc: "Gets the current status of the market close or open".to_string(),
                    wss: false,
                },
                option_info: ApiEndpoint {
                    base: "https://www.nseindia.com/api/option-chain-contract-info".to_string(),
                    params: Some(vec![
                        ApiParam { key: "symbol".to_string(), value: "FINNIFTY".to_string(), dynamic: true, param_key: Some("fno_symbol".to_string()) },
                    ]),
                    desc: "Fetch the expiry dates and strike prices for the index".to_string(),
                    wss: false,
                },
                option_ticks: ApiEndpoint {
                    base: "wss://streamer.nseindia.com/streams/fo/mbp".to_string(),
                    params: Some(vec![
                        ApiParam { key: "symbol".to_string(), value: "FINNIFTY".to_string(), dynamic: true, param_key: Some("fno_symbol".to_string()) },
                        ApiParam { key: "expiry".to_string(), value: "30-Jun-2026".to_string(), dynamic: true, param_key: Some("expiry_date".to_string()) },
                    ]),
                    desc: "Websocket to get CE and PE price movements".to_string(),
                    wss: true,
                },
                indices_info: ApiEndpoint {
                    base: "https://www.nseindia.com/api/NextApi/apiClient/indexTrackerApi".to_string(),
                    params: Some(vec![
                        ApiParam { key: "functionName".to_string(), value: "getAllIndices".to_string(), dynamic: false, param_key: None },
                    ]),
                    desc: "Get F&O index name, underlying index names, short and long names".to_string(),
                    wss: false,
                },
                index_info: ApiEndpoint {
                    base: "https://www.nseindia.com/api/NextApi/apiClient/indexTrackerApi".to_string(),
                    params: Some(vec![
                        ApiParam { key: "functionName".to_string(), value: "getIndexData".to_string(), dynamic: false, param_key: None },
                        ApiParam { key: "index".to_string(), value: "NIFTY 50".to_string(), dynamic: true, param_key: Some("index_display_name".to_string()) },
                    ]),
                    desc: "Get high level status for the index".to_string(),
                    wss: false,
                },
                index_chart: ApiEndpoint {
                    base: "https://www.nseindia.com/api/NextApi/apiClient/indexTrackerApi".to_string(),
                    params: Some(vec![
                        ApiParam { key: "functionName".to_string(), value: "getIndexChart".to_string(), dynamic: false, param_key: None },
                        ApiParam { key: "index".to_string(), value: "NIFTY 50".to_string(), dynamic: true, param_key: Some("index_display_name".to_string()) },
                        ApiParam { key: "flag".to_string(), value: "1D".to_string(), dynamic: true, param_key: Some("time_range_flag".to_string()) },
                    ]),
                    desc: "Get index price movements for 1D flag".to_string(),
                    wss: false,
                },
                historic_index_chart: ApiEndpoint {
                    base: "https://www.nseindia.com/api/NextApi/apiClient/historicalGraph".to_string(),
                    params: Some(vec![
                        ApiParam { key: "functionName".to_string(), value: "getIndexChart".to_string(), dynamic: false, param_key: None },
                        ApiParam { key: "index".to_string(), value: "NIFTY 50".to_string(), dynamic: true, param_key: Some("index_display_name".to_string()) },
                        ApiParam { key: "flag".to_string(), value: "1M".to_string(), dynamic: true, param_key: Some("time_range_flag".to_string()) },
                    ]),
                    desc: "Get historic index price movements for a specific time range flag".to_string(),
                    wss: false,
                },
                indices_streamer: ApiEndpoint {
                    base: "wss://streamer.nseindia.com/streams/indices/high/windices".to_string(),
                    params: None,
                    desc: "WebSocket stream for real-time index data".to_string(),
                    wss: true,
                },
            },
            user: UserConfig {
                valid_symbols: vec![
                    "NIFTY".to_string(),
                    "NIFTYNXT50".to_string(),
                    "FINNIFTY".to_string(),
                    "BANKNIFTY".to_string(),
                    "MIDCPNIFTY".to_string(),
                ],
                default_symbol: "NIFTY".to_string(),
                default_time_range_flag: "1M".to_string(),
            },
        }
    }
}

pub fn setup_app_folders() -> io::Result<AppPaths> {

    let base_path = dirs::data_local_dir()
        .or_else(|| env::current_dir().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Could not determine a storage location"))?;

    // 2. Construct Paths
    let root = base_path.join(format!(".{}", APP_NAME));
    let models = root.join("models");
    let chats_dir = root.join("chats");
    let db_dir = root.join("db");
    let settings_file = root.join("settings.json");
    let model_names_file = models.join("names.json");

    let paths = AppPaths {
        stt: models.join("stt"),
        llm: models.join("llm"),
        tts: models.join("tts"),
        models: models,
        chats_dir,
        db: db_dir,
        settings_file,
        model_names_file,
        root: root,
    };

    // 3. Create Directories
    // We only need to call create_dir_all on the "deepest" leaf nodes;
    // it will automatically create 'root' and 'models' as parents.
    fs::create_dir_all(&paths.root)?;
    fs::create_dir_all(&paths.models)?;
    fs::create_dir_all(&paths.stt)?;
    fs::create_dir_all(&paths.llm)?;
    fs::create_dir_all(&paths.tts)?;
    fs::create_dir_all(&paths.chats_dir)?;
    fs::create_dir_all(&paths.db)?;
    Ok(paths)
}

pub fn load_or_create_config(settings_file: &PathBuf) -> io::Result<AppConfig> {
    // Check if settings file exists
    if settings_file.exists() {
        match fs::read_to_string(settings_file) {
            Ok(content) => {
                match serde_json::from_str::<AppConfig>(&content) {
                    Ok(config) => {
                        info!("✅ Loaded existing configuration from: {}", settings_file.display());
                        return Ok(config);
                    }
                    Err(e) => {
                        error!("⚠️ Failed to parse settings.json: {}. Using defaults.", e);
                        // Fall through to create default
                    }
                }
            }
            Err(e) => {
                error!("⚠️ Failed to read settings.json: {}. Using defaults.", e);
                // Fall through to create default
            }
        }
    }

    // Create default config and save it
    let default_config = AppConfig::default();
    let config_json = serde_json::to_string_pretty(&default_config)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    fs::write(settings_file, &config_json)?;
    info!("✅ Created default configuration at: {}", settings_file.display());
    Ok(default_config)
}