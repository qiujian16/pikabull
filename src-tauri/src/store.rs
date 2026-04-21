use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;

static DB: std::sync::OnceLock<Mutex<Connection>> = std::sync::OnceLock::new();

const INTRADAY_TTL_HOURS: i64 = 4;

#[derive(Debug, Clone)]
pub struct PriceRow {
    pub date: String,
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume: f64,
    pub pct_change: f64,
    pub amount: f64,
}

fn db_path() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("pikabull");
    std::fs::create_dir_all(&path).ok();
    path.push("price_cache.db");
    path
}

fn get_conn() -> &'static Mutex<Connection> {
    DB.get_or_init(|| {
        let path = db_path();
        let conn = Connection::open(&path).expect("Failed to open SQLite database");
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS price_data (
                 symbol      TEXT NOT NULL,
                 date        TEXT NOT NULL,
                 open        REAL,
                 high        REAL,
                 low         REAL,
                 close       REAL NOT NULL,
                 volume      REAL,
                 pct_change  REAL,
                 amount      REAL,
                 fetched_at  TEXT NOT NULL,
                 PRIMARY KEY (symbol, date)
             );
             CREATE INDEX IF NOT EXISTS idx_price_symbol_date ON price_data (symbol, date);
             CREATE TABLE IF NOT EXISTS coverage (
                 symbol      TEXT NOT NULL,
                 start_date  TEXT NOT NULL,
                 end_date    TEXT NOT NULL,
                 row_count   INTEGER NOT NULL DEFAULT 0,
                 fetched_at  TEXT NOT NULL,
                 PRIMARY KEY (symbol, start_date, end_date)
             );
             CREATE INDEX IF NOT EXISTS idx_coverage_symbol ON coverage (symbol);",
        )
        .expect("Failed to create database tables");
        println!("[price_store] DB ready at {}", path.display());
        Mutex::new(conn)
    })
}

fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

fn is_covered(conn: &Connection, symbol: &str, start_date: &str, end_date: &str) -> bool {
    let is_historical = end_date < today().as_str();

    if is_historical {
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM coverage WHERE symbol=? AND start_date<=? AND end_date>=?",
                params![symbol, start_date, end_date],
                |row| row.get(0),
            )
            .unwrap_or(0);
        count > 0
    } else {
        let cutoff = (Utc::now() - Duration::hours(INTRADAY_TTL_HOURS))
            .to_rfc3339();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM coverage WHERE symbol=? AND start_date<=? AND end_date>=? AND fetched_at>=?",
                params![symbol, start_date, end_date, cutoff],
                |row| row.get(0),
            )
            .unwrap_or(0);
        count > 0
    }
}

pub fn load(symbol: &str, start_date: &str, end_date: &str) -> Option<Vec<PriceRow>> {
    let conn = get_conn().lock().ok()?;

    if !is_covered(&conn, symbol, start_date, end_date) {
        return None;
    }

    let mut stmt = conn
        .prepare(
            "SELECT date, open, close, high, low, volume, pct_change, amount \
             FROM price_data WHERE symbol=? AND date BETWEEN ? AND ? ORDER BY date",
        )
        .ok()?;

    let rows: Vec<PriceRow> = stmt
        .query_map(params![symbol, start_date, end_date], |row| {
            Ok(PriceRow {
                date: row.get(0)?,
                open: row.get(1)?,
                close: row.get(2)?,
                high: row.get(3)?,
                low: row.get(4)?,
                volume: row.get(5)?,
                pct_change: row.get(6)?,
                amount: row.get(7)?,
            })
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        None
    } else {
        println!(
            "[price_store] Local hit for {} {}→{} ({} rows)",
            symbol,
            start_date,
            end_date,
            rows.len()
        );
        Some(rows)
    }
}

pub fn upsert(symbol: &str, start_date: &str, end_date: &str, rows: &[PriceRow]) {
    let conn = match get_conn().lock() {
        Ok(c) => c,
        Err(_) => return,
    };

    let now = Utc::now().to_rfc3339();
    let tx = match conn.unchecked_transaction() {
        Ok(tx) => tx,
        Err(_) => return,
    };

    for row in rows {
        tx.execute(
            "INSERT OR REPLACE INTO price_data \
             (symbol, date, open, high, low, close, volume, pct_change, amount, fetched_at) \
             VALUES (?,?,?,?,?,?,?,?,?,?)",
            params![
                symbol,
                row.date,
                row.open,
                row.high,
                row.low,
                row.close,
                row.volume,
                row.pct_change,
                row.amount,
                now
            ],
        )
        .ok();
    }

    tx.execute(
        "INSERT OR REPLACE INTO coverage \
         (symbol, start_date, end_date, row_count, fetched_at) \
         VALUES (?,?,?,?,?)",
        params![symbol, start_date, end_date, rows.len() as i32, now],
    )
    .ok();

    tx.commit().ok();
}
