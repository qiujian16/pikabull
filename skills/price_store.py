"""SQLite-backed local store for China A-share daily price data.

Schema
------
price_data  — one OHLCV row per (symbol, date), upserted on every fetch
coverage    — one record per successfully fetched (symbol, start_date, end_date)
              lets us answer "do we already have this range?" in O(1)

Threading
---------
Each thread gets its own sqlite3.Connection via threading.local().
A module-level Lock serialises all writes so concurrent thread-pool workers
(from asyncio run_in_executor) never collide.

Staleness
---------
Historical ranges (end_date < today): trusted forever — prices don't change.
Ranges that include today: stale after INTRADAY_TTL_HOURS hours.
"""
from __future__ import annotations

import sqlite3
import threading
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

import pandas as pd

# ── Paths ──────────────────────────────────────────────────────────────────────

_DB_DIR = Path(__file__).parent.parent / "data"
_DB_PATH = _DB_DIR / "price_cache.db"

INTRADAY_TTL_HOURS = 4   # re-fetch if today's data is older than this

# ── Connection pool (one connection per thread) ────────────────────────────────

_thread_local = threading.local()
_write_lock = threading.Lock()


def _conn() -> sqlite3.Connection:
    if not hasattr(_thread_local, "conn"):
        _DB_DIR.mkdir(parents=True, exist_ok=True)
        c = sqlite3.connect(str(_DB_PATH), check_same_thread=False)
        c.execute("PRAGMA journal_mode=WAL")
        c.execute("PRAGMA synchronous=NORMAL")
        c.row_factory = sqlite3.Row
        _thread_local.conn = c
    return _thread_local.conn


# ── Schema ─────────────────────────────────────────────────────────────────────

_DDL = """
CREATE TABLE IF NOT EXISTS price_data (
    symbol      TEXT    NOT NULL,
    date        TEXT    NOT NULL,   -- YYYY-MM-DD
    open        REAL,
    high        REAL,
    low         REAL,
    close       REAL    NOT NULL,
    volume      REAL,
    pct_change  REAL,
    amount      REAL,
    source      TEXT,
    fetched_at  TEXT    NOT NULL,
    PRIMARY KEY (symbol, date)
);

CREATE INDEX IF NOT EXISTS idx_price_symbol_date
    ON price_data (symbol, date);

CREATE TABLE IF NOT EXISTS coverage (
    symbol      TEXT    NOT NULL,
    start_date  TEXT    NOT NULL,
    end_date    TEXT    NOT NULL,
    row_count   INTEGER NOT NULL DEFAULT 0,
    source      TEXT,
    fetched_at  TEXT    NOT NULL,
    PRIMARY KEY (symbol, start_date, end_date)
);

CREATE INDEX IF NOT EXISTS idx_coverage_symbol
    ON coverage (symbol);
"""


def init_db(db_path: str | None = None) -> None:
    """Create tables/indexes if absent. Call once at app startup."""
    global _DB_PATH
    if db_path:
        _DB_PATH = Path(db_path)
    _DB_DIR.mkdir(parents=True, exist_ok=True)
    c = _conn()
    c.executescript(_DDL)
    c.commit()
    print(f"[price_store] DB ready at {_DB_PATH}")


# ── Helpers ────────────────────────────────────────────────────────────────────

def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _today() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%d")


# ── Read ───────────────────────────────────────────────────────────────────────

def is_covered(
    symbol: str,
    start_date: str,
    end_date: str,
    max_age_hours: float | None = None,
) -> bool:
    """True if (symbol, start_date, end_date) is in the coverage table and not stale."""
    c = _conn()

    # Historical data: never goes stale
    if max_age_hours is None and end_date < _today():
        max_age_hours = None   # no staleness check
    elif end_date >= _today():
        max_age_hours = INTRADAY_TTL_HOURS

    if max_age_hours is None:
        row = c.execute(
            "SELECT 1 FROM coverage WHERE symbol=? AND start_date<=? AND end_date>=? LIMIT 1",
            (symbol, start_date, end_date),
        ).fetchone()
    else:
        from datetime import timedelta
        cutoff = (
            datetime.now(timezone.utc) - timedelta(hours=max_age_hours)
        ).isoformat()
        row = c.execute(
            """SELECT 1 FROM coverage
               WHERE symbol=? AND start_date<=? AND end_date>=?
                 AND fetched_at >= ?
               LIMIT 1""",
            (symbol, start_date, end_date, cutoff),
        ).fetchone()

    return row is not None


def load(symbol: str, start_date: str, end_date: str) -> pd.DataFrame | None:
    """Return local rows for (symbol, start_date..end_date), or None if empty."""
    c = _conn()
    rows = c.execute(
        """SELECT date, open, close, high, low, volume, pct_change, amount, source
           FROM price_data
           WHERE symbol=? AND date BETWEEN ? AND ?
           ORDER BY date""",
        (symbol, start_date, end_date),
    ).fetchall()
    if not rows:
        return None
    df = pd.DataFrame(rows, columns=["date", "open", "close", "high", "low",
                                     "volume", "pct_change", "amount", "source"])
    for col in ("open", "close", "high", "low", "volume", "pct_change", "amount"):
        df[col] = pd.to_numeric(df[col], errors="coerce")
    return df


# ── Write ──────────────────────────────────────────────────────────────────────

def upsert(
    df: pd.DataFrame,
    symbol: str,
    start_date: str,
    end_date: str,
    source: str = "unknown",
) -> None:
    """Insert-or-replace rows into price_data and record coverage."""
    now = _now_iso()
    rows = [
        (
            symbol,
            row["date"],
            row.get("open"),
            row.get("high"),
            row.get("low"),
            row["close"],
            row.get("volume"),
            row.get("pct_change"),
            row.get("amount"),
            source,
            now,
        )
        for _, row in df.iterrows()
    ]
    with _write_lock:
        c = _conn()
        c.executemany(
            """INSERT OR REPLACE INTO price_data
               (symbol, date, open, high, low, close, volume, pct_change, amount, source, fetched_at)
               VALUES (?,?,?,?,?,?,?,?,?,?,?)""",
            rows,
        )
        c.execute(
            """INSERT OR REPLACE INTO coverage
               (symbol, start_date, end_date, row_count, source, fetched_at)
               VALUES (?,?,?,?,?,?)""",
            (symbol, start_date, end_date, len(df), source, now),
        )
        c.commit()


# ── Composite entry point ──────────────────────────────────────────────────────

def query_or_fetch(
    symbol: str,
    start_date: str,
    end_date: str,
    fetcher_fn: Callable[[str, str, str], pd.DataFrame] | None = None,
) -> pd.DataFrame:
    """
    Return OHLCV data for symbol+range.
    Serves from local DB when the range is covered; fetches from network otherwise.
    """
    from .price_fetcher import _fmt_date
    sd = _fmt_date(start_date)
    ed = _fmt_date(end_date)

    if is_covered(symbol, sd, ed):
        df = load(symbol, sd, ed)
        if df is not None and not df.empty:
            print(f"[price_store] Local hit for {symbol} {sd}→{ed} ({len(df)} rows)")
            return df

    print(f"[price_store] Fetching from network: {symbol} {sd}→{ed}")
    if fetcher_fn is None:
        from .price_fetcher import fetch_price_df as fetcher_fn
    df = fetcher_fn(symbol, sd, ed)

    source = df["source"].iloc[0] if "source" in df.columns else "unknown"
    upsert(df, symbol, sd, ed, source=source)
    return df


# ── Maintenance ────────────────────────────────────────────────────────────────

def purge_before(cutoff_date: str) -> int:
    """Delete rows older than cutoff_date. Returns number of price_data rows deleted."""
    with _write_lock:
        c = _conn()
        cur = c.execute("DELETE FROM price_data WHERE date < ?", (cutoff_date,))
        c.execute("DELETE FROM coverage WHERE end_date < ?", (cutoff_date,))
        c.commit()
    return cur.rowcount


def db_stats() -> dict:
    """Return basic stats about the local cache."""
    c = _conn()
    row = c.execute(
        "SELECT COUNT(DISTINCT symbol), COUNT(*), MIN(date), MAX(date) FROM price_data"
    ).fetchone()
    size = _DB_PATH.stat().st_size if _DB_PATH.exists() else 0
    return {
        "symbols": row[0],
        "total_rows": row[1],
        "oldest_date": row[2],
        "newest_date": row[3],
        "db_size_bytes": size,
        "db_path": str(_DB_PATH),
    }
