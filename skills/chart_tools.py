"""Plotly chart generation for China A-share stocks.

Returns the chart as a JSON-serialisable dict so the frontend can call
Plotly.react(div, data, layout) directly — no server-side image rendering needed.
"""
from __future__ import annotations

import json

import numpy as np
import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from .price_store import query_or_fetch as fetch_price_df


def generate_stock_chart(
    symbol: str,
    stock_name: str,
    start_date: str,
    end_date: str,
) -> dict | None:
    """
    Build a 3-panel chart:
      Row 1 (60%): Candlestick + SMA20/50 + volume bars
      Row 2 (20%): MACD histogram + lines
      Row 3 (20%): RSI with overbought/oversold bands

    Returns a Plotly figure serialised to a plain dict, or None on error.
    """
    try:
        df = fetch_price_df(symbol, start_date, end_date)
        if df.empty:
            return None

        # ── Indicators ─────────────────────────────────────────────────────────
        df["sma20"] = df["close"].rolling(20).mean()
        df["sma50"] = df["close"].rolling(50).mean()

        ema12 = df["close"].ewm(span=12, adjust=False).mean()
        ema26 = df["close"].ewm(span=26, adjust=False).mean()
        df["macd"] = ema12 - ema26
        df["macd_signal"] = df["macd"].ewm(span=9, adjust=False).mean()
        df["macd_hist"] = df["macd"] - df["macd_signal"]

        delta = df["close"].diff()
        gain = delta.clip(lower=0).rolling(14).mean()
        loss = (-delta.clip(upper=0)).rolling(14).mean()
        df["rsi"] = 100 - 100 / (1 + gain / loss)

        # ── Colour helpers ─────────────────────────────────────────────────────
        # A-share convention: red = up, green = down
        price_colors = [
            "#ef5350" if p >= 0 else "#26a69a" for p in df["pct_change"]
        ]
        macd_colors = [
            "#ef5350" if v >= 0 else "#26a69a"
            for v in df["macd_hist"].fillna(0)
        ]

        # ── Layout ─────────────────────────────────────────────────────────────
        fig = make_subplots(
            rows=3,
            cols=1,
            shared_xaxes=True,
            vertical_spacing=0.04,
            row_heights=[0.58, 0.22, 0.20],
            subplot_titles=(
                f"{stock_name}（{symbol}）",
                "MACD (12,26,9)",
                "RSI (14)",
            ),
        )

        # ── Row 1: Candlestick ─────────────────────────────────────────────────
        fig.add_trace(
            go.Candlestick(
                x=df["date"],
                open=df["open"],
                high=df["high"],
                low=df["low"],
                close=df["close"],
                name="Price",
                increasing_line_color="#ef5350",
                decreasing_line_color="#26a69a",
                increasing_fillcolor="#ef5350",
                decreasing_fillcolor="#26a69a",
            ),
            row=1,
            col=1,
        )
        fig.add_trace(
            go.Scatter(
                x=df["date"], y=df["sma20"], name="SMA20",
                line=dict(color="#ff9800", width=1.2),
            ),
            row=1, col=1,
        )
        fig.add_trace(
            go.Scatter(
                x=df["date"], y=df["sma50"], name="SMA50",
                line=dict(color="#2196f3", width=1.2),
            ),
            row=1, col=1,
        )
        # Volume as secondary y on row 1 (normalised to ~15% of price range)
        price_range = df["high"].max() - df["low"].min()
        vol_scale = price_range * 0.15 / df["volume"].max() if df["volume"].max() > 0 else 1
        fig.add_trace(
            go.Bar(
                x=df["date"],
                y=df["volume"] * vol_scale + df["low"].min(),
                name="Volume",
                marker_color=price_colors,
                opacity=0.4,
                showlegend=False,
            ),
            row=1, col=1,
        )

        # ── Row 2: MACD ────────────────────────────────────────────────────────
        fig.add_trace(
            go.Bar(
                x=df["date"], y=df["macd_hist"],
                name="MACD Hist", marker_color=macd_colors, opacity=0.8,
            ),
            row=2, col=1,
        )
        fig.add_trace(
            go.Scatter(
                x=df["date"], y=df["macd"], name="MACD",
                line=dict(color="#2196f3", width=1.2),
            ),
            row=2, col=1,
        )
        fig.add_trace(
            go.Scatter(
                x=df["date"], y=df["macd_signal"], name="Signal",
                line=dict(color="#ff9800", width=1.2),
            ),
            row=2, col=1,
        )

        # ── Row 3: RSI ─────────────────────────────────────────────────────────
        fig.add_trace(
            go.Scatter(
                x=df["date"], y=df["rsi"], name="RSI",
                line=dict(color="#9c27b0", width=1.5),
                fill="tozeroy",
                fillcolor="rgba(156,39,176,0.08)",
            ),
            row=3, col=1,
        )
        for level, color in ((70, "rgba(239,83,80,0.5)"), (30, "rgba(38,166,154,0.5)")):
            fig.add_hline(y=level, line_dash="dot", line_color=color, row=3, col=1)

        # ── Styling ────────────────────────────────────────────────────────────
        fig.update_layout(
            height=620,
            template="plotly_white",
            showlegend=True,
            legend=dict(
                orientation="h", yanchor="bottom", y=1.02,
                xanchor="right", x=1, font=dict(size=11),
            ),
            margin=dict(l=55, r=20, t=60, b=40),
            xaxis_rangeslider_visible=False,
            plot_bgcolor="#fafafa",
        )
        fig.update_yaxes(title_text="Price (CNY)", row=1, col=1, showgrid=True)
        fig.update_yaxes(title_text="MACD", row=2, col=1, showgrid=True)
        fig.update_yaxes(title_text="RSI", row=3, col=1, range=[0, 100], showgrid=True)

        return json.loads(fig.to_json())

    except Exception as e:
        print(f"[chart_tools] Error generating chart for {symbol}: {e}")
        return None
