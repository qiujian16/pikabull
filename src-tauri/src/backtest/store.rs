use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::engine::BacktestResult;

fn extract_metrics(result_json: &str) -> (f64, u32) {
    let v: serde_json::Value = match serde_json::from_str(result_json) {
        Ok(v) => v,
        Err(_) => return (0.0, 0),
    };
    let m = &v["metrics"];
    let ret = m["totalReturnPct"]
        .as_f64()
        .or_else(|| m["total_return_pct"].as_f64())
        .unwrap_or(0.0);
    let trades = m["totalTrades"]
        .as_u64()
        .or_else(|| m["total_trades"].as_u64())
        .unwrap_or(0) as u32;
    (ret, trades)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestRecord {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub strategy_name: String,
    pub strategy_json: String,
    pub start_date: String,
    pub end_date: String,
    pub initial_capital: f64,
    pub result_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestMeta {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub strategy_name: String,
    pub start_date: String,
    pub end_date: String,
    pub total_return_pct: f64,
    pub total_trades: u32,
    pub created_at: String,
}

pub fn save(
    symbol: &str,
    name: &str,
    config: &super::engine::BacktestConfig,
    result: &BacktestResult,
) -> String {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let strategy_json = serde_json::to_string(&config.strategy).unwrap_or_default();
    let result_json = serde_json::to_string(result).unwrap_or_default();

    let conn = crate::config_store::get_conn().lock().unwrap();
    conn.execute(
        "INSERT INTO backtest_runs \
         (id, symbol, name, strategy_name, strategy_json, start_date, end_date, \
          initial_capital, commission_rate, stamp_tax_rate, result_json, created_at) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            id,
            symbol,
            name,
            config.strategy.name,
            strategy_json,
            config.start_date,
            config.end_date,
            config.initial_capital,
            config.commission_rate,
            config.stamp_tax_rate,
            result_json,
            now,
        ],
    )
    .ok();

    id
}

pub fn get(id: &str) -> Option<BacktestRecord> {
    let conn = crate::config_store::get_conn().lock().ok()?;
    conn.query_row(
        "SELECT id, symbol, name, strategy_name, strategy_json, start_date, end_date, \
         initial_capital, result_json, created_at \
         FROM backtest_runs WHERE id=?",
        params![id],
        |row| {
            Ok(BacktestRecord {
                id: row.get(0)?,
                symbol: row.get(1)?,
                name: row.get(2)?,
                strategy_name: row.get(3)?,
                strategy_json: row.get(4)?,
                start_date: row.get(5)?,
                end_date: row.get(6)?,
                initial_capital: row.get(7)?,
                result_json: row.get(8)?,
                created_at: row.get(9)?,
            })
        },
    )
    .ok()
}

pub fn list() -> Vec<BacktestMeta> {
    let conn = match crate::config_store::get_conn().lock() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut stmt = match conn.prepare(
        "SELECT id, symbol, name, strategy_name, start_date, end_date, result_json, created_at \
         FROM backtest_runs ORDER BY created_at DESC",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |row| {
        let result_json: String = row.get(6)?;
        let (total_return_pct, total_trades) = extract_metrics(&result_json);
        Ok(BacktestMeta {
            id: row.get(0)?,
            symbol: row.get(1)?,
            name: row.get(2)?,
            strategy_name: row.get(3)?,
            start_date: row.get(4)?,
            end_date: row.get(5)?,
            total_return_pct,
            total_trades,
            created_at: row.get(7)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

pub fn delete(id: &str) {
    let conn = match crate::config_store::get_conn().lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    conn.execute("DELETE FROM backtest_runs WHERE id=?", params![id])
        .ok();
}
