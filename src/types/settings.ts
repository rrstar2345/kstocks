// Mirrors the `AppSettings` struct returned by the `get_app_settings` Tauri
// command (src-tauri/src/lib.rs), which is sourced from settings.json via
// `settings.rs`. Keep this in sync with that struct.
export interface AppSettings {
  index_chart_refresh_interval_seconds: number;
  cards_fallback_poll_interval_seconds: number;
  time_range_flags: string[];
  default_time_range_flag: string;
}