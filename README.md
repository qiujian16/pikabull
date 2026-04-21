# PikaBull ⚡🐂

> Inspired by [TauricResearch/TradingAgents](https://github.com/TauricResearch/TradingAgents) — re-implemented for China A-share markets as a native desktop app with Tauri + Vue 3, a provider-agnostic LLM layer, and real-time streaming UI.

A multi-agent stock analysis and strategy backtesting desktop app for China A-share markets.

**Analysis:** Pick one or more stocks, set a date range, and watch eight specialised agents work in sequence — results stream to the UI in real time.

**Backtesting:** Define trading strategies via preset templates or natural language (translated by LLM), and replay them over historical data with a deterministic engine that enforces A-share rules (T+1 settlement, 100-share lots, commission + stamp tax).

---

## How it works

### Multi-agent analysis

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
   BUY / HOLD / SELL  +  full report in app
```

### Strategy backtesting

```
User picks stock + date range + strategy
          │
          ▼
┌──────────────────────────────────────────────────────┐
│  Strategy input (choose one):                        │
│    • Preset templates with parameter sliders         │
│    • Natural language → LLM translates to formal     │
│      strategy struct with validation retry loop      │
├──────────────────────────────────────────────────────┤
│  Deterministic engine:                               │
│    1. Fetch historical OHLCV data                    │
│    2. Pre-compute indicators (SMA, EMA, RSI, MACD,   │
│       Bollinger Bands, volume)                       │
│    3. Walk bars: evaluate entry/exit conditions,     │
│       stop loss, take profit, trailing stop          │
│    4. Enforce A-share rules (T+1, 100-share lots)   │
└──────────────────────────────────────────────────────┘
          │
          ▼
   Metrics + equity curve + trade log
   (total/annualized return, Sharpe, max drawdown,
    win rate, profit factor, benchmark comparison)
```

**5 preset strategies:** Golden Cross (SMA), RSI Mean Reversion, MACD Momentum, Bollinger Bounce, Dual MA + RSI Filter. Each has adjustable parameters via sliders.

**Natural language input:** Describe a strategy in plain language (e.g. "RSI低于30且价格在60日均线上方时买入，RSI超过70时卖出") and the LLM translates it to a formal strategy struct with a validation retry loop.

**Real-time quotes** (market indices, watchlist prices) come from Sina Finance. **Analysis data** (historical klines, financials, news) is fetched from Eastmoney and persisted locally in a SQLite database. Repeat analyses of the same stock and date range are served instantly from disk — no network call.

**LLM providers** are plug-and-play: Anthropic (Claude), OpenAI (GPT), MiniMax, or any Ollama-compatible local model. All agent prompts use the OpenAI message format internally. Providers are configured through the in-app settings panel — no config files needed.

---

## Quick start

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install) toolchain (via `rustup`)
- An API key for at least one LLM provider (configured in-app)

### 1. Clone and install

```bash
git clone <repo-url>
cd pikabull
npm install
```

### 2. Run

```bash
make dev
```

The app will compile the Rust backend and launch the desktop window. On first launch, open the settings panel to configure your LLM provider and API key.

### 3. Build release

```bash
make build
```

This produces a `.dmg` installer (macOS) or platform-appropriate package under `src-tauri/target/release/bundle/`.

### Available make targets

| Command | Description |
|---|---|
| `make dev` | Run in dev mode with hot reload |
| `make build` | Build release `.dmg` / `.app` |
| `make check` | Type-check frontend + Rust |
| `make fmt` | Format Rust + frontend code |
| `make lint` | Run clippy on Rust |
| `make clean` | Remove all build artifacts |
| `make install` | Install npm + cargo dependencies |
| `make open` | Open the built `.dmg` (macOS) |

---

## Configuration

### LLM providers

LLM providers are configured through the in-app settings panel. You can add multiple provider configurations and switch between them.

| Provider | Required fields |
|---|---|
| Anthropic (Claude) | API key |
| OpenAI | API key |
| MiniMax | API key |
| Ollama / Hermes (local) | Base URL, model name |

### Price data cache

Fetched OHLCV data is stored in a SQLite database under your OS data directory (e.g. `~/Library/Application Support/pikabull/price_cache.db` on macOS).

- **Historical ranges** (end date before today) are cached permanently — no re-fetch ever
- **Ranges including today** are re-fetched after 4 hours
- To clear the cache: delete the `price_cache.db` file

---

## Tech stack

| Layer | Technology |
|---|---|
| Desktop shell | [Tauri v2](https://tauri.app/) |
| Backend | Rust (reqwest, rusqlite, tokio, serde) |
| Frontend | Vue 3 + TypeScript + Vite |
| Charts | Plotly.js |
| Markdown | marked.js |
| Real-time data | Sina Finance API |
| Analysis data | Eastmoney HTTP APIs |
| LLM APIs | Anthropic / OpenAI-compatible (via reqwest) |

---

## Project structure

```
pikabull/
├── src/                        Vue 3 frontend
│   ├── App.vue                 Main UI: sidebar, search, analysis display
│   ├── BacktestView.vue        Backtest UI: presets, NL input, results, history
│   ├── main.ts                 Vue app entry point
│   └── vite-env.d.ts           TypeScript declarations
│
├── src-tauri/                  Rust backend (Tauri v2)
│   ├── src/
│   │   ├── lib.rs              Tauri setup, command registration
│   │   ├── main.rs             Entry point
│   │   ├── commands.rs         Tauri commands (analysis, backtest, config, watchlist)
│   │   ├── store.rs            SQLite price cache (query_or_fetch, coverage tracking)
│   │   ├── config_store.rs     SQLite config/settings/backtest persistence
│   │   │
│   │   ├── providers/
│   │   │   ├── mod.rs          LLMProvider trait, types, provider factory
│   │   │   ├── anthropic.rs    Anthropic Claude API (complete + SSE streaming)
│   │   │   └── openai.rs       OpenAI-compatible API (also Ollama, MiniMax)
│   │   │
│   │   ├── agents/
│   │   │   ├── mod.rs
│   │   │   ├── base.rs         Provider-agnostic tool-use loop + streaming
│   │   │   ├── workflow.rs     8-step analysis pipeline, emits Tauri events
│   │   │   └── strategy_translator.rs  NL → Strategy via LLM tool-use
│   │   │
│   │   ├── backtest/
│   │   │   ├── mod.rs
│   │   │   ├── strategy.rs     Tagged-enum strategy schema (17 indicator conditions)
│   │   │   ├── engine.rs       Deterministic backtest engine with indicator cache
│   │   │   ├── metrics.rs      Performance metrics (Sharpe, drawdown, win rate, etc.)
│   │   │   ├── presets.rs      5 preset strategies with adjustable parameters
│   │   │   └── store.rs        SQLite CRUD for backtest run history
│   │   │
│   │   └── skills/
│   │       ├── mod.rs          Tool schemas (OpenAI function format) + executor
│   │       ├── stock_data.rs   Sina (quotes) + Eastmoney (klines, financials, news, search)
│   │       ├── indicators.rs   SMA, EMA, RSI, MACD, Bollinger Bands (pure Rust)
│   │       └── chart.rs        Plotly JSON builder: candlestick + MACD + RSI
│   │
│   ├── Cargo.toml              Rust dependencies
│   └── tauri.conf.json         Tauri app configuration
│
├── Makefile                    Build commands (dev, build, check, fmt, lint, clean)
├── package.json                Node.js dependencies
├── vite.config.ts              Vite build configuration
└── tsconfig.json               TypeScript configuration
```

---

## What's next

### Data
- [ ] Intraday data — add minute-bar support using Eastmoney's intraday API
- [ ] Fundamental cache — store stock info and financial data locally; they change only quarterly
- [ ] Cache management UI — sidebar panel showing DB size, symbols cached, oldest/newest dates

### Analysis
- [ ] Sector / index context — add a market agent that fetches CSI 300 or SSE Composite data for macro context
- [x] Backtesting — strategy-based backtest engine with preset templates, NL strategy translation, and full results UI
- [ ] Portfolio view — analyse multiple stocks and produce a combined allocation recommendation

### Infrastructure
- [x] Streaming for market/fundamental/news agents
- [ ] Cancellation — wire up a stop button that cancels in-progress analysis tasks
- [ ] Multi-window — open each stock analysis in its own Tauri window
- [ ] Auto-update — use Tauri's built-in updater for seamless version upgrades
