mod agents;
mod backtest;
mod commands;
mod config_store;
mod providers;
mod skills;
mod store;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::search_stocks,
            commands::get_market_indices,
            commands::get_provider_info,
            commands::start_analysis,
            commands::list_model_configs,
            commands::add_model_config,
            commands::update_model_config,
            commands::delete_model_config,
            commands::set_active_model,
            commands::get_active_model,
            commands::get_watchlist,
            commands::add_to_watchlist,
            commands::remove_from_watchlist,
            commands::get_watchlist_quotes,
            commands::save_analysis_report,
            commands::get_saved_report,
            commands::list_report_metas,
            commands::run_backtest,
            commands::list_backtests,
            commands::get_backtest,
            commands::delete_backtest,
            commands::get_preset_strategies,
            commands::translate_strategy,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
