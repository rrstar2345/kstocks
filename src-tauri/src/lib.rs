pub mod nse;
pub mod db;
pub mod settings;

use db::{init_db, start_tick_writer};
use settings::{setup_app_folders};
use nse::OptionStreamer;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
async fn store_ticks(symbol: String) -> String {
    let paths = setup_app_folders()
        .expect("failed to setup app folders");

    let db_file = paths.db.join("ticks.sqlite");
    let pool = init_db(&db_file).await.expect("failed to init db");

    let (tx, writer_handle) = start_tick_writer(pool);

    // run writer task
    tokio::spawn(async move {
        let _ = writer_handle.await;
    });

    // Example: start streaming
    let expiry = "30-Jun-2026".to_string();

    let streamer = OptionStreamer::new(symbol.clone(), expiry, tx);
    let _ = streamer.start().await;

    format!("Data fetch started for {}", symbol)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![store_ticks])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
