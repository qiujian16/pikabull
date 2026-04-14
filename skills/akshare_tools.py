"""akshare-based tool definitions and implementations.

Tool schemas use the OpenAI function-calling format so they work with any
provider (Anthropic, OpenAI, Ollama/Hermes, etc.).
"""
from __future__ import annotations

import akshare as ak
import pandas as pd
import numpy as np

from .price_store import query_or_fetch

# ── Tool Schemas (OpenAI function-calling format) ──────────────────────────────

MARKET_TOOLS: list[dict] = [
    {
        "type": "function",
        "function": {
            "name": "get_stock_history",
            "description": (
                "Fetch historical daily OHLCV data for a China A-share stock from akshare. "
                "Returns date, open, close, high, low, volume, pct_change for the requested period."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "6-digit A-share code, e.g. '000001' (Ping An Bank), '600519' (Moutai)",
                    },
                    "start_date": {
                        "type": "string",
                        "description": "Start date YYYYMMDD, e.g. '20240101'",
                    },
                    "end_date": {
                        "type": "string",
                        "description": "End date YYYYMMDD, e.g. '20241231'",
                    },
                },
                "required": ["symbol", "start_date", "end_date"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "get_technical_indicators",
            "description": (
                "Calculate technical indicators from akshare price data. "
                "Available: sma20, sma50, ema10, rsi14, macd, macd_signal, macd_hist, "
                "boll_upper, boll_mid, boll_lower."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "symbol": {"type": "string", "description": "6-digit A-share code"},
                    "start_date": {"type": "string", "description": "Start date YYYYMMDD"},
                    "end_date": {"type": "string", "description": "End date YYYYMMDD"},
                    "indicators": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": (
                            "Indicator names to compute. "
                            "Choose from: sma20, sma50, ema10, rsi14, macd, macd_signal, "
                            "macd_hist, boll_upper, boll_mid, boll_lower"
                        ),
                    },
                },
                "required": ["symbol", "start_date", "end_date", "indicators"],
            },
        },
    },
]

FUNDAMENTAL_TOOLS: list[dict] = [
    {
        "type": "function",
        "function": {
            "name": "get_stock_info",
            "description": (
                "Get basic real-time information for an A-share stock: PE, PB, "
                "market cap, industry, 52-week high/low, etc."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "symbol": {"type": "string", "description": "6-digit A-share code"},
                },
                "required": ["symbol"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "get_financial_data",
            "description": (
                "Get annual financial summary for an A-share company: revenue, net profit, "
                "ROE, debt ratio, EPS from recent annual reports."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "symbol": {"type": "string", "description": "6-digit A-share code"},
                },
                "required": ["symbol"],
            },
        },
    },
]

NEWS_TOOLS: list[dict] = [
    {
        "type": "function",
        "function": {
            "name": "get_stock_news",
            "description": "Fetch recent news articles and announcements for an A-share stock.",
            "parameters": {
                "type": "object",
                "properties": {
                    "symbol": {"type": "string", "description": "6-digit A-share code"},
                    "limit": {
                        "type": "integer",
                        "description": "Max number of articles to return (default 20)",
                    },
                },
                "required": ["symbol"],
            },
        },
    },
]

# ── Implementations ────────────────────────────────────────────────────────────

def _fetch_price_df(symbol: str, start_date: str, end_date: str) -> pd.DataFrame:
    """Return normalised price DataFrame; serves from local DB when available."""
    return query_or_fetch(symbol, start_date, end_date)


def get_stock_history(symbol: str, start_date: str, end_date: str) -> str:
    try:
        df = _fetch_price_df(symbol, start_date, end_date)
        if df.empty:
            return f"No price data found for {symbol}"

        latest = df.iloc[-1]
        summary = (
            f"Stock: {symbol}\n"
            f"Period: {df['date'].iloc[0]} → {df['date'].iloc[-1]}  ({len(df)} trading days)\n"
            f"Latest close: {latest['close']:.2f}  change: {latest['pct_change']:.2f}%\n"
            f"Period high: {df['high'].max():.2f}  low: {df['low'].min():.2f}\n"
            f"Avg daily volume: {df['volume'].mean():.0f}\n\n"
            f"Recent 20 trading days:\n"
            f"{df[['date','open','close','high','low','volume','pct_change']].tail(20).to_string(index=False)}"
        )
        return summary
    except Exception as e:
        return f"Error fetching stock history for {symbol}: {e}"


def get_technical_indicators(
    symbol: str, start_date: str, end_date: str, indicators: list[str]
) -> str:
    try:
        df = _fetch_price_df(symbol, start_date, end_date)
        if df.empty:
            return f"No price data found for {symbol}"

        out = df[["date", "close"]].copy()

        if "sma20" in indicators:
            out["sma20"] = df["close"].rolling(20).mean()
        if "sma50" in indicators:
            out["sma50"] = df["close"].rolling(50).mean()
        if "ema10" in indicators:
            out["ema10"] = df["close"].ewm(span=10, adjust=False).mean()
        if "rsi14" in indicators:
            delta = df["close"].diff()
            gain = delta.clip(lower=0).rolling(14).mean()
            loss = (-delta.clip(upper=0)).rolling(14).mean()
            out["rsi14"] = 100 - 100 / (1 + gain / loss)
        if any(x in indicators for x in ("macd", "macd_signal", "macd_hist")):
            ema12 = df["close"].ewm(span=12, adjust=False).mean()
            ema26 = df["close"].ewm(span=26, adjust=False).mean()
            macd_line = ema12 - ema26
            signal = macd_line.ewm(span=9, adjust=False).mean()
            if "macd" in indicators:
                out["macd"] = macd_line
            if "macd_signal" in indicators:
                out["macd_signal"] = signal
            if "macd_hist" in indicators:
                out["macd_hist"] = macd_line - signal
        if any(x in indicators for x in ("boll_upper", "boll_mid", "boll_lower")):
            mid = df["close"].rolling(20).mean()
            std = df["close"].rolling(20).std()
            if "boll_upper" in indicators:
                out["boll_upper"] = mid + 2 * std
            if "boll_mid" in indicators:
                out["boll_mid"] = mid
            if "boll_lower" in indicators:
                out["boll_lower"] = mid - 2 * std

        return (
            f"Technical indicators for {symbol} ({start_date} → {end_date}):\n"
            f"{out.tail(30).round(3).to_string(index=False)}"
        )
    except Exception as e:
        return f"Error computing indicators for {symbol}: {e}"


def get_stock_info(symbol: str) -> str:
    try:
        df = ak.stock_individual_info_em(symbol=symbol)
        return f"Stock info for {symbol}:\n{df.to_string(index=False)}"
    except Exception as e:
        return f"Error fetching stock info for {symbol}: {e}"


def get_financial_data(symbol: str) -> str:
    try:
        df = ak.stock_financial_abstract_ths(symbol=symbol, indicator="按年度")
        return f"Annual financial data for {symbol}:\n{df.head(5).to_string(index=False)}"
    except Exception as e:
        # Fallback: try balance sheet summary
        try:
            df = ak.stock_balance_sheet_by_yearly_em(symbol=symbol)
            return f"Balance sheet (yearly) for {symbol}:\n{df.head(3).to_string(index=False)}"
        except Exception as e2:
            return f"Error fetching financial data for {symbol}: {e} / {e2}"


def get_stock_news(symbol: str, limit: int = 20) -> str:
    try:
        df = ak.stock_news_em(symbol=symbol)
        df = df.head(limit)

        # Detect column names defensively
        cols = df.columns.tolist()
        title_col = next((c for c in cols if "标题" in c), cols[0])
        date_col = next((c for c in cols if "时间" in c or "日期" in c), None)
        content_col = next((c for c in cols if "内容" in c), None)

        lines = [f"Recent news for {symbol}:\n"]
        for _, row in df.iterrows():
            lines.append(f"【{row[title_col]}】")
            if date_col:
                lines.append(f"  时间: {row[date_col]}")
            if content_col:
                snippet = str(row[content_col])[:200]
                lines.append(f"  摘要: {snippet}…")
            lines.append("")
        return "\n".join(lines)
    except Exception as e:
        return f"Error fetching news for {symbol}: {e}"


# ── Tool executor ──────────────────────────────────────────────────────────────

_DISPATCH: dict[str, callable] = {
    "get_stock_history": get_stock_history,
    "get_technical_indicators": get_technical_indicators,
    "get_stock_info": get_stock_info,
    "get_financial_data": get_financial_data,
    "get_stock_news": get_stock_news,
}


def execute_tool(tool_name: str, tool_input: dict) -> str:
    """Dispatch a tool call by name. Used by the provider-agnostic agent loop."""
    fn = _DISPATCH.get(tool_name)
    if fn is None:
        return f"Unknown tool: {tool_name}"
    try:
        return fn(**tool_input)
    except Exception as e:
        return f"Tool '{tool_name}' error: {e}"
