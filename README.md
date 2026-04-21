# PikaBull ⚡🐂

> Inspired by [TauricResearch/TradingAgents](https://github.com/TauricResearch/TradingAgents) — re-implemented for China A-share markets as a native desktop app with Tauri + Vue 3, a provider-agnostic LLM layer, and real-time streaming UI.

A multi-agent stock analysis desktop app for China A-share markets. Pick one or more stocks, set a date range, and watch eight specialised agents work in sequence: chart generation → technical analysis → fundamental analysis → news sentiment → bull/bear debate → trading decision → risk assessment.

Results stream to the UI in real time as each agent finishes.

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
   BUY / HOLD / SELL  +  full report in app
```

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
│   ├── main.ts                 Vue app entry point
│   └── vite-env.d.ts           TypeScript declarations
│
├── src-tauri/                  Rust backend (Tauri v2)
│   ├── src/
│   │   ├── lib.rs              Tauri setup, command registration
│   │   ├── main.rs             Entry point
│   │   ├── commands.rs         Tauri commands: search, analyze, provider info
│   │   ├── store.rs            SQLite price cache (query_or_fetch, coverage tracking)
│   │   │
│   │   ├── providers/
│   │   │   ├── mod.rs          LLMProvider trait, types, provider factory
│   │   │   ├── anthropic.rs    Anthropic Claude API (complete + SSE streaming)
│   │   │   └── openai.rs       OpenAI-compatible API (also Ollama, MiniMax)
│   │   │
│   │   ├── agents/
│   │   │   ├── mod.rs
│   │   │   ├── base.rs         Provider-agnostic tool-use loop + streaming
│   │   │   └── workflow.rs     8-step analysis pipeline, emits Tauri events
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
- [ ] Backtesting — replay the workflow over a sliding window and score signals against actual returns
- [ ] Portfolio view — analyse multiple stocks and produce a combined allocation recommendation

### Infrastructure
- [x] Streaming for market/fundamental/news agents
- [ ] Cancellation — wire up a stop button that cancels in-progress analysis tasks
- [ ] Multi-window — open each stock analysis in its own Tauri window
- [ ] Auto-update — use Tauri's built-in updater for seamless version upgrades
