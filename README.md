# PikaBull ⚡🐂

> Inspired by [TauricResearch/TradingAgents](https://github.com/TauricResearch/TradingAgents) — re-implemented for China A-share markets with a provider-agnostic LLM layer, akshare data, and a streaming web UI instead of LangChain/LangGraph.

A multi-agent stock analysis system for China A-share markets. Pick one or more stocks, set a date range, and watch eight specialised agents work in sequence: chart generation → technical analysis → fundamental analysis → news sentiment → bull/bear debate → trading decision → risk assessment.

Results stream to the browser in real time as each agent finishes.

---

## How it works

```
User picks stock + date range
          │
          ▼
┌─────────────────────────────────────────────────────┐
│  1. Chart          Candlestick + MACD + RSI (Plotly)│
│  2. Market         Technical analysis (tools)        │
│  3. Fundamental    Financials & valuation (tools)    │  ← parallel
│  4. News           Sentiment from recent articles    │
│  5. Bull           Pro-buy argument (streaming)      │  ← waterfall
│  6. Bear           Pro-sell argument (streaming)     │
│  7. Trader         Investment decision (streaming)   │
│  8. Risk Manager   Risk assessment & final call      │
└─────────────────────────────────────────────────────┘
          │
          ▼
   BUY / HOLD / SELL  +  full report in browser
```

**Price data** is fetched once and persisted locally in a SQLite database (`data/price_cache.db`). Repeat analyses of the same stock and date range are served instantly from disk — no network call.

**LLM providers** are plug-and-play: Anthropic (Claude), OpenAI (GPT), MiniMax, or any Ollama-compatible local model. All agent prompts use the OpenAI message format internally, so switching providers requires only a `.env` change.

---

## Quick start

### Prerequisites

- Python 3.11+
- [uv](https://github.com/astral-sh/uv) — `curl -LsSf https://astral.sh/uv/install.sh | sh`
- An API key for at least one LLM provider

### 1. Clone and run

```bash
git clone <repo-url>
cd pikabull
./start.sh
```

On first run, `start.sh` will:
1. Create a `.venv` with uv
2. Install all dependencies from `requirements.txt`
3. Copy `.env.example` → `.env` and exit, asking you to fill in your API key

### 2. Configure `.env`

Open `.env` and set your provider and key:

```dotenv
# Choose one provider
LLM_PROVIDER=anthropic          # anthropic | openai | minimax | ollama

# Then set the matching key/model
ANTHROPIC_API_KEY=sk-ant-...
ANTHROPIC_MODEL=claude-sonnet-4-6
```

See `.env.example` for all provider options.

### 3. Start again

```bash
./start.sh
```

Open [http://127.0.0.1:8000](http://127.0.0.1:8000) in your browser.

---

## Configuration

### LLM providers

| Provider | `LLM_PROVIDER` | Required env vars |
|---|---|---|
| Anthropic (Claude) | `anthropic` | `ANTHROPIC_API_KEY` |
| OpenAI | `openai` | `OPENAI_API_KEY` |
| MiniMax | `minimax` | `MINIMAX_API_KEY` |
| Ollama / Hermes (local) | `ollama` | `OLLAMA_BASE_URL`, `OLLAMA_MODEL` |

### Server

```dotenv
HOST=127.0.0.1    # bind address (use 0.0.0.0 to expose on LAN)
PORT=8000         # port
```

Set these in your shell before running `start.sh`, or export them.

### Price data cache

Fetched OHLCV data is stored in `data/price_cache.db` (SQLite, gitignored).

- **Historical ranges** (end date before today) are cached permanently — no re-fetch ever
- **Ranges including today** are re-fetched after 4 hours
- To clear the cache: `rm data/price_cache.db`

### Network / proxy

If your environment has an HTTP proxy, the price fetcher automatically clears proxy env vars before connecting to Baostock (which uses a raw TCP socket, not HTTP). Yahoo Finance is used as a fallback and goes through the normal proxy.

---

## Project structure

```
pikabull/
├── app.py                  FastAPI app, SSE endpoint, stock search
├── start.sh                One-command setup and launch (uv)
├── requirements.txt
├── .env.example
│
├── agents/
│   ├── workflow.py         8-step orchestration pipeline (async generator)
│   ├── base_agent.py       Provider-agnostic tool-use loop + streaming helper
│   └── __init__.py
│
├── providers/
│   ├── base.py             BaseLLMProvider ABC (complete + stream_complete)
│   ├── anthropic_provider.py
│   ├── openai_provider.py  Also used for MiniMax and Ollama
│   └── __init__.py         create_provider_from_env() factory
│
├── skills/
│   ├── price_store.py      SQLite cache layer (query_or_fetch entry point)
│   ├── price_fetcher.py    Network layer: Baostock → Yahoo Finance fallback
│   ├── akshare_tools.py    Tool schemas + implementations (history, indicators,
│   │                       stock info, financials, news) via akshare
│   ├── chart_tools.py      Plotly candlestick + MACD + RSI chart builder
│   └── __init__.py
│
├── templates/
│   └── index.html          Bootstrap 5 + Plotly.js + marked.js UI
│
└── data/
    └── price_cache.db      Auto-created SQLite DB (gitignored)
```

---

## What's next

### Data
- [ ] **Intraday data** — add minute-bar tables to `price_store` using Baostock's `frequency="5"` or `"30"` queries
- [ ] **Fundamental cache** — store `get_stock_info` and `get_financial_data` results locally; they change only quarterly
- [ ] **Cache management UI** — add an `/api/cache/stats` endpoint and a sidebar panel showing DB size, symbols cached, oldest/newest dates

### Analysis
- [ ] **Sector / index context** — add a market agent that fetches CSI 300 or SSE Composite data and includes macro context in the bull/bear debate
- [ ] **Backtesting** — replay the workflow over a sliding window of historical dates and score BUY/HOLD/SELL signals against actual returns
- [ ] **Portfolio view** — analyse multiple stocks and produce a combined allocation recommendation

### Infrastructure
- [ ] **Streaming for market/fundamental/news agents** — currently these three run to completion before results appear; add `stream_complete` to them as well
- [ ] **Scheduled refresh** — a background task that re-fetches today's data for watched stocks at market close
- [ ] **Authentication** — add a simple API key gate if exposing the app outside localhost
- [ ] **Docker image** — single-container deployment with `data/` volume mount for persistence
