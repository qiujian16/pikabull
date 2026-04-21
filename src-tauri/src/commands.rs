use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

use crate::agents::{strategy_translator, workflow};
use crate::backtest;
use crate::config_store;
use crate::providers;
use crate::skills::stock_data;

#[derive(Debug, Serialize, Deserialize)]
pub struct StockResult {
    pub code: String,
    pub name: String,
}

#[tauri::command]
pub fn search_stocks(query: String) -> Vec<StockResult> {
    stock_data::search_stocks(&query)
        .into_iter()
        .map(|(code, name)| StockResult { code, name })
        .collect()
}

#[tauri::command]
pub fn get_market_indices() -> Vec<stock_data::MarketIndex> {
    stock_data::get_market_indices()
}

#[tauri::command]
pub fn get_provider_info() -> providers::ProviderInfo {
    if let Some(config) = config_store::get_active() {
        return providers::ProviderInfo {
            provider: config.provider,
            model: config.model,
        };
    }
    providers::get_provider_info()
}

#[tauri::command]
pub fn list_model_configs() -> Vec<config_store::ModelConfig> {
    config_store::list()
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_model_config(
    name: String,
    provider: String,
    model: String,
    api_key: String,
    base_url: String,
) -> config_store::ModelConfig {
    config_store::add(&name, &provider, &model, &api_key, &base_url)
}

#[tauri::command(rename_all = "camelCase")]
pub fn update_model_config(
    id: String,
    name: String,
    provider: String,
    model: String,
    api_key: String,
    base_url: String,
) {
    config_store::update(&id, &name, &provider, &model, &api_key, &base_url);
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_model_config(id: String) {
    config_store::delete(&id);
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_active_model(id: String) {
    config_store::set_active(&id);
}

#[tauri::command]
pub fn get_active_model() -> Option<config_store::ModelConfig> {
    config_store::get_active()
}

// ── Watchlist ──

#[tauri::command]
pub fn get_watchlist() -> Vec<config_store::WatchlistItem> {
    config_store::watchlist_list()
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_to_watchlist(symbol: String, name: String) {
    config_store::watchlist_add(&symbol, &name);
}

#[tauri::command]
pub fn remove_from_watchlist(symbol: String) {
    config_store::watchlist_remove(&symbol);
}

#[tauri::command]
pub fn get_watchlist_quotes(symbols: Vec<String>) -> Vec<stock_data::StockQuote> {
    stock_data::get_stock_quotes(&symbols)
}

// ── Saved Reports ──

#[tauri::command(rename_all = "camelCase")]
pub fn save_analysis_report(
    symbol: String,
    name: String,
    start_date: String,
    end_date: String,
    decision: String,
    chart_data: Option<String>,
    report_data: String,
) {
    config_store::save_report(
        &symbol,
        &name,
        &start_date,
        &end_date,
        &decision,
        chart_data.as_deref(),
        &report_data,
    );
}

#[tauri::command]
pub fn get_saved_report(symbol: String) -> Option<config_store::SavedReport> {
    config_store::get_report(&symbol)
}

#[tauri::command]
pub fn list_report_metas() -> Vec<config_store::ReportMeta> {
    config_store::list_report_metas()
}

// ── Analysis ──

#[tauri::command(rename_all = "camelCase")]
pub async fn start_analysis(
    app: AppHandle,
    symbols: Vec<String>,
    start_date: String,
    end_date: String,
    enabled_agents: Vec<String>,
) -> Result<(), String> {
    let provider = providers::create_active_provider()?;
    let provider: Arc<dyn providers::LLMProvider> = Arc::from(provider);

    for symbol in symbols {
        let app_clone = app.clone();
        let provider_clone = provider.clone();
        let sd = start_date.clone();
        let ed = end_date.clone();
        let enabled = enabled_agents.clone();

        tokio::spawn(async move {
            workflow::analyze_stock(app_clone, provider_clone, &symbol, &sd, &ed, enabled).await;
        });
    }

    Ok(())
}

// ── Backtest ──

#[tauri::command(rename_all = "camelCase")]
pub async fn run_backtest(
    app: AppHandle,
    symbol: String,
    name: String,
    start_date: String,
    end_date: String,
    strategy_json: String,
    initial_capital: f64,
    commission_rate: Option<f64>,
    stamp_tax_rate: Option<f64>,
) -> Result<String, String> {
    let strategy: backtest::strategy::Strategy =
        serde_json::from_str(&strategy_json).map_err(|e| format!("Invalid strategy JSON: {e}"))?;

    let config = backtest::engine::BacktestConfig {
        symbol: symbol.clone(),
        start_date: start_date.clone(),
        end_date: end_date.clone(),
        initial_capital,
        strategy,
        commission_rate: commission_rate.unwrap_or(0.0003),
        stamp_tax_rate: stamp_tax_rate.unwrap_or(0.001),
    };

    let _ = app.emit("backtest-progress", json!({"stage": "fetching", "percent": 10}));

    let rows = tokio::task::spawn_blocking({
        let symbol = symbol.clone();
        let sd = start_date.clone();
        let ed = end_date.clone();
        move || stock_data::fetch_price_data(&symbol, &sd, &ed)
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))?
    .map_err(|e| format!("Failed to fetch price data: {e}"))?;

    let _ = app.emit("backtest-progress", json!({"stage": "computing", "percent": 40, "bars": rows.len()}));

    let result = backtest::engine::run_backtest(&config, &rows)?;

    let _ = app.emit("backtest-progress", json!({"stage": "saving", "percent": 80}));

    let id = tokio::task::spawn_blocking({
        let name = name.clone();
        let symbol = symbol.clone();
        let config = config.clone();
        let result = result.clone();
        move || backtest::store::save(&symbol, &name, &config, &result)
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))?;

    let _ = app.emit("backtest-progress", json!({"stage": "complete", "percent": 100}));

    let mut response = serde_json::to_value(&result).map_err(|e| format!("Serialize error: {e}"))?;
    response["id"] = serde_json::Value::String(id);
    serde_json::to_string(&response).map_err(|e| format!("Serialize error: {e}"))
}

#[tauri::command]
pub fn list_backtests() -> Vec<backtest::store::BacktestMeta> {
    backtest::store::list()
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_backtest(id: String) -> Option<backtest::store::BacktestRecord> {
    backtest::store::get(&id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_backtest(id: String) {
    backtest::store::delete(&id);
}

#[tauri::command]
pub fn get_preset_strategies() -> Vec<backtest::presets::PresetStrategy> {
    backtest::presets::list_presets()
}

#[tauri::command]
pub async fn translate_strategy(description: String) -> Result<String, String> {
    let provider = providers::create_active_provider()?;

    let (strategy, explanation) =
        strategy_translator::translate_strategy(provider.as_ref(), &description).await?;

    let result = serde_json::json!({
        "strategy": strategy,
        "explanation": explanation,
    });

    serde_json::to_string(&result).map_err(|e| format!("Serialize error: {e}"))
}
