use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

static DB: std::sync::OnceLock<Mutex<Connection>> = std::sync::OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

fn db_path() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("pikabull");
    std::fs::create_dir_all(&path).ok();
    path.push("price_cache.db");
    path
}

pub(crate) fn get_conn() -> &'static Mutex<Connection> {
    DB.get_or_init(|| {
        let path = db_path();
        let conn = Connection::open(&path).expect("Failed to open config database");
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS model_configs (
                 id        TEXT PRIMARY KEY,
                 name      TEXT NOT NULL,
                 provider  TEXT NOT NULL,
                 model     TEXT NOT NULL,
                 api_key   TEXT NOT NULL DEFAULT '',
                 base_url  TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE IF NOT EXISTS app_settings (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS watchlist (
                 symbol     TEXT PRIMARY KEY,
                 name       TEXT NOT NULL,
                 sort_order INTEGER NOT NULL DEFAULT 0,
                 added_at   TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS saved_reports (
                 symbol      TEXT PRIMARY KEY,
                 name        TEXT NOT NULL,
                 start_date  TEXT NOT NULL,
                 end_date    TEXT NOT NULL,
                 decision    TEXT NOT NULL DEFAULT '',
                 chart_data  TEXT,
                 report_data TEXT NOT NULL,
                 analyzed_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS data_cache (
                 key        TEXT PRIMARY KEY,
                 data       TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS backtest_runs (
                 id              TEXT PRIMARY KEY,
                 symbol          TEXT NOT NULL,
                 name            TEXT NOT NULL,
                 strategy_name   TEXT NOT NULL,
                 strategy_json   TEXT NOT NULL,
                 start_date      TEXT NOT NULL,
                 end_date        TEXT NOT NULL,
                 initial_capital REAL NOT NULL,
                 commission_rate REAL NOT NULL,
                 stamp_tax_rate  REAL NOT NULL,
                 result_json     TEXT NOT NULL,
                 created_at      TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_backtest_symbol ON backtest_runs (symbol);
             CREATE INDEX IF NOT EXISTS idx_backtest_created ON backtest_runs (created_at);",
        )
        .expect("Failed to create config tables");
        Mutex::new(conn)
    })
}

fn row_to_config(row: &rusqlite::Row) -> rusqlite::Result<ModelConfig> {
    Ok(ModelConfig {
        id: row.get(0)?,
        name: row.get(1)?,
        provider: row.get(2)?,
        model: row.get(3)?,
        api_key: row.get(4)?,
        base_url: row.get(5)?,
    })
}

pub fn list() -> Vec<ModelConfig> {
    let conn = match get_conn().lock() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut stmt = match conn.prepare(
        "SELECT id, name, provider, model, api_key, base_url FROM model_configs ORDER BY name",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], row_to_config)
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

pub fn add(
    name: &str,
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
) -> ModelConfig {
    let id = Uuid::new_v4().to_string();
    let conn = get_conn().lock().unwrap();
    conn.execute(
        "INSERT INTO model_configs (id, name, provider, model, api_key, base_url) VALUES (?,?,?,?,?,?)",
        params![id, name, provider, model, api_key, base_url],
    )
    .ok();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM model_configs", [], |row| row.get(0))
        .unwrap_or(0);
    if count == 1 {
        conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value) VALUES ('active_model_id', ?)",
            params![id],
        )
        .ok();
    }

    ModelConfig {
        id,
        name: name.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        api_key: api_key.to_string(),
        base_url: base_url.to_string(),
    }
}

pub fn update(
    id: &str,
    name: &str,
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
) {
    let conn = get_conn().lock().unwrap();
    conn.execute(
        "UPDATE model_configs SET name=?, provider=?, model=?, api_key=?, base_url=? WHERE id=?",
        params![name, provider, model, api_key, base_url, id],
    )
    .ok();
}

pub fn delete(id: &str) {
    let conn = get_conn().lock().unwrap();
    conn.execute("DELETE FROM model_configs WHERE id=?", params![id])
        .ok();
    let active: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key='active_model_id'",
            [],
            |row| row.get(0),
        )
        .ok();
    if active.as_deref() == Some(id) {
        conn.execute(
            "DELETE FROM app_settings WHERE key='active_model_id'",
            [],
        )
        .ok();
    }
}

pub fn get_active_id() -> Option<String> {
    let conn = get_conn().lock().ok()?;
    conn.query_row(
        "SELECT value FROM app_settings WHERE key='active_model_id'",
        [],
        |row| row.get(0),
    )
    .ok()
}

pub fn set_active(id: &str) {
    let conn = get_conn().lock().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO app_settings (key, value) VALUES ('active_model_id', ?)",
        params![id],
    )
    .ok();
}

pub fn get_active() -> Option<ModelConfig> {
    let id = get_active_id()?;
    let conn = get_conn().lock().ok()?;
    conn.query_row(
        "SELECT id, name, provider, model, api_key, base_url FROM model_configs WHERE id=?",
        params![id],
        row_to_config,
    )
    .ok()
}

// ── Watchlist ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchlistItem {
    pub symbol: String,
    pub name: String,
    pub sort_order: i32,
    pub added_at: String,
}

pub fn watchlist_list() -> Vec<WatchlistItem> {
    let conn = match get_conn().lock() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut stmt = match conn
        .prepare("SELECT symbol, name, sort_order, added_at FROM watchlist ORDER BY sort_order")
    {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |row| {
        Ok(WatchlistItem {
            symbol: row.get(0)?,
            name: row.get(1)?,
            sort_order: row.get(2)?,
            added_at: row.get(3)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

pub fn watchlist_add(symbol: &str, name: &str) {
    let conn = get_conn().lock().unwrap();
    let max_order: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) FROM watchlist",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO watchlist (symbol, name, sort_order, added_at) VALUES (?,?,?,?)",
        params![symbol, name, max_order + 1, now],
    )
    .ok();
}

pub fn watchlist_remove(symbol: &str) {
    let conn = get_conn().lock().unwrap();
    conn.execute("DELETE FROM watchlist WHERE symbol=?", params![symbol])
        .ok();
}

// ── Saved Reports ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedReport {
    pub symbol: String,
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub decision: String,
    pub chart_data: Option<String>,
    pub report_data: String,
    pub analyzed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMeta {
    pub symbol: String,
    pub decision: String,
    pub analyzed_at: String,
}

pub fn save_report(
    symbol: &str,
    name: &str,
    start_date: &str,
    end_date: &str,
    decision: &str,
    chart_data: Option<&str>,
    report_data: &str,
) {
    let conn = get_conn().lock().unwrap();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO saved_reports \
         (symbol, name, start_date, end_date, decision, chart_data, report_data, analyzed_at) \
         VALUES (?,?,?,?,?,?,?,?)",
        params![symbol, name, start_date, end_date, decision, chart_data, report_data, now],
    )
    .ok();
}

pub fn get_report(symbol: &str) -> Option<SavedReport> {
    let conn = get_conn().lock().ok()?;
    conn.query_row(
        "SELECT symbol, name, start_date, end_date, decision, chart_data, report_data, analyzed_at \
         FROM saved_reports WHERE symbol=?",
        params![symbol],
        |row| {
            Ok(SavedReport {
                symbol: row.get(0)?,
                name: row.get(1)?,
                start_date: row.get(2)?,
                end_date: row.get(3)?,
                decision: row.get(4)?,
                chart_data: row.get(5)?,
                report_data: row.get(6)?,
                analyzed_at: row.get(7)?,
            })
        },
    )
    .ok()
}

pub fn list_report_metas() -> Vec<ReportMeta> {
    let conn = match get_conn().lock() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut stmt = match conn
        .prepare("SELECT symbol, decision, analyzed_at FROM saved_reports")
    {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |row| {
        Ok(ReportMeta {
            symbol: row.get(0)?,
            decision: row.get(1)?,
            analyzed_at: row.get(2)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

// ── Data Cache ──

pub fn cache_set(key: &str, data: &str) {
    let conn = match get_conn().lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO data_cache (key, data, updated_at) VALUES (?,?,?)",
        params![key, data, now],
    )
    .ok();
}

pub fn cache_get(key: &str) -> Option<String> {
    let conn = get_conn().lock().ok()?;
    conn.query_row(
        "SELECT data FROM data_cache WHERE key=?",
        params![key],
        |row| row.get(0),
    )
    .ok()
}
