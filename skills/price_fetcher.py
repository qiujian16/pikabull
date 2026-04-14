"""Shared price data fetcher with multi-source fallback.

Priority:
  1. Baostock – socket-based (api.baostock.com:9527); proxy env vars cleared before connect
  2. Yahoo Finance (yfinance) – HTTPS to US servers; works from any network

Results are cached in-process so repeated calls for the same symbol+range are free
and concurrent requests never hit the same source simultaneously.
"""
from __future__ import annotations

import os
import socket
import threading
import time

import pandas as pd
import yfinance as yf

_PROXY_VARS = ("http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY",
               "all_proxy", "ALL_PROXY")

# Baostock has a single global TCP connection — serialise all access
_baostock_lock = threading.Lock()

# Simple in-process cache: key = "symbol:start:end"
_price_cache: dict[str, pd.DataFrame] = {}
_cache_lock = threading.Lock()


def _fmt_date(d: str) -> str:
    d = d.replace("-", "")
    return f"{d[:4]}-{d[4:6]}-{d[6:]}"


def _yf_ticker(symbol: str) -> str:
    return f"{symbol}.SS" if symbol.startswith(("6", "9")) else f"{symbol}.SZ"


def _exchange_prefix(symbol: str) -> str:
    return "sh" if symbol.startswith(("6", "9")) else "sz"


# ── Baostock ───────────────────────────────────────────────────────────────────

def _from_baostock(symbol: str, start_date: str, end_date: str,
                   timeout: int = 15) -> pd.DataFrame:
    import baostock as bs

    code = f"{_exchange_prefix(symbol)}.{symbol}"

    saved_proxy = {v: os.environ.pop(v) for v in _PROXY_VARS if v in os.environ}
    old_timeout = socket.getdefaulttimeout()
    socket.setdefaulttimeout(timeout)
    try:
        with _baostock_lock:
            lg = bs.login()
            if lg.error_code != "0":
                raise RuntimeError(f"Baostock login failed: {lg.error_msg}")
            try:
                rs = bs.query_history_k_data_plus(
                    code,
                    "date,open,high,low,close,volume,turn,pctChg",
                    start_date=start_date,
                    end_date=end_date,
                    frequency="d",
                    adjustflag="2",
                )
                if rs.error_code != "0":
                    raise RuntimeError(f"Baostock query failed: {rs.error_msg}")
                rows = []
                while rs.next():
                    rows.append(rs.get_row_data())
            finally:
                bs.logout()
    finally:
        socket.setdefaulttimeout(old_timeout)
        os.environ.update(saved_proxy)

    if not rows:
        raise RuntimeError(f"Baostock returned no data for {symbol}")

    df = pd.DataFrame(rows, columns=rs.fields)
    for col in ("open", "high", "low", "close", "volume"):
        df[col] = pd.to_numeric(df[col], errors="coerce")
    df["pct_change"] = pd.to_numeric(df.get("pctChg", 0), errors="coerce")
    df["amount"] = 0
    df = df.dropna(subset=["close"])
    return df[["date", "open", "close", "high", "low", "volume", "pct_change", "amount"]]


# ── Yahoo Finance ──────────────────────────────────────────────────────────────

def _from_yfinance(symbol: str, start_date: str, end_date: str,
                   retries: int = 3) -> pd.DataFrame:
    ticker = _yf_ticker(symbol)
    ed = (pd.to_datetime(end_date) + pd.Timedelta(days=1)).strftime("%Y-%m-%d")

    last_err: Exception | None = None
    for attempt in range(retries):
        try:
            df = yf.download(ticker, start=start_date, end=ed,
                             auto_adjust=True, progress=False)
            if not df.empty:
                break
            last_err = RuntimeError(f"Yahoo Finance returned empty data for {ticker}")
        except Exception as exc:
            last_err = exc
        if attempt < retries - 1:
            wait = 3 * (2 ** attempt)   # 3s, 6s
            print(f"[price_fetcher] Yahoo retry {attempt + 1}/{retries} in {wait}s: {last_err}")
            time.sleep(wait)
    else:
        raise last_err

    df = df.reset_index()
    df.columns = [c[0] if isinstance(c, tuple) else c for c in df.columns]
    df = df.rename(columns={
        "Date": "date", "Open": "open", "High": "high",
        "Low": "low", "Close": "close", "Volume": "volume",
    })
    df["date"] = pd.to_datetime(df["date"]).dt.strftime("%Y-%m-%d")
    df["pct_change"] = df["close"].pct_change() * 100
    df["amount"] = 0
    return df[["date", "open", "close", "high", "low", "volume", "pct_change", "amount"]]


# ── Public API ─────────────────────────────────────────────────────────────────

def fetch_price_df(symbol: str, start_date: str, end_date: str) -> pd.DataFrame:
    """Return normalised OHLCV DataFrame; cached, Baostock → Yahoo Finance."""
    sd = _fmt_date(start_date)
    ed = _fmt_date(end_date)
    key = f"{symbol}:{sd}:{ed}"

    with _cache_lock:
        if key in _price_cache:
            print(f"[price_fetcher] Cache hit for {symbol}")
            return _price_cache[key].copy()

    e_bs: Exception | None = None
    try:
        df = _from_baostock(symbol, sd, ed)
        print(f"[price_fetcher] Baostock OK for {symbol} ({len(df)} rows)")
    except Exception as exc:
        e_bs = exc
        print(f"[price_fetcher] Baostock failed for {symbol}: {exc}")
        try:
            df = _from_yfinance(symbol, sd, ed)
            print(f"[price_fetcher] Yahoo Finance OK for {symbol} ({len(df)} rows)")
        except Exception as e_yf:
            raise RuntimeError(
                f"All price sources failed for {symbol}: Baostock={e_bs}; Yahoo={e_yf}"
            )

    with _cache_lock:
        _price_cache[key] = df
    return df.copy()
