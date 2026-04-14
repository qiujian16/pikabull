"""PikaBull — FastAPI application.

Routes:
  GET  /                    Web UI
  GET  /api/stocks/search   Stock autocomplete (akshare A-share list)
  GET  /api/analyze         SSE stream: full multi-agent analysis
  GET  /api/provider        Current provider info
"""
from __future__ import annotations

import asyncio
import json
import os
from contextlib import asynccontextmanager

import akshare as ak
from dotenv import load_dotenv
from fastapi import FastAPI, Query, Request
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates
from sse_starlette.sse import EventSourceResponse

load_dotenv()

# ── Bypass proxy for Chinese financial data sources used by akshare ────────────
# akshare pulls from eastmoney, sina, xueqiu, etc. If the system has a proxy
# configured (HTTP_PROXY / HTTPS_PROXY), those domestic hosts often fail.
# Adding them to NO_PROXY makes requests connect directly.
_AKSHARE_NO_PROXY = (
    # Explicit hosts + leading-dot forms so subdomains are also matched
    "eastmoney.com,.eastmoney.com,"
    "sina.com.cn,.sina.com.cn,hq.sinajs.cn,finance.sina.com.cn,"
    "xueqiu.com,.xueqiu.com,"
    "10jqka.com.cn,.10jqka.com.cn,jqka.com.cn,.jqka.com.cn,"
    "sse.com.cn,.sse.com.cn,szse.cn,.szse.cn,"
    "gtimg.cn,.gtimg.cn,localhost,127.0.0.1"
)
for _var in ("no_proxy", "NO_PROXY"):
    _cur = os.environ.get(_var, "")
    os.environ[_var] = f"{_cur},{_AKSHARE_NO_PROXY}" if _cur else _AKSHARE_NO_PROXY

# ── Stock list cache ───────────────────────────────────────────────────────────
_stock_list: list[dict] = []


@asynccontextmanager
async def lifespan(app: FastAPI):
    global _stock_list
    loop = asyncio.get_event_loop()
    from skills.price_store import init_db
    init_db()

    try:
        df = await loop.run_in_executor(None, ak.stock_info_a_code_name)
        _stock_list = df.to_dict("records")
        print(f"[startup] Loaded {len(_stock_list)} A-share stocks")
    except Exception as e:
        print(f"[startup] Warning: could not load stock list: {e}")
    yield


app = FastAPI(title="PikaBull", lifespan=lifespan)
templates = Jinja2Templates(directory="templates")


# ── Routes ─────────────────────────────────────────────────────────────────────

@app.get("/", response_class=HTMLResponse)
async def index(request: Request):
    return templates.TemplateResponse(request=request, name="index.html")


@app.get("/api/provider")
async def get_provider_info():
    """Return which provider is configured (for the UI status bar)."""
    provider = os.getenv("LLM_PROVIDER", "anthropic")
    if provider == "anthropic":
        model = os.getenv("ANTHROPIC_MODEL", "claude-sonnet-4-6")
    elif provider == "openai":
        model = os.getenv("OPENAI_MODEL", "gpt-4o")
    else:  # ollama
        model = os.getenv("OLLAMA_MODEL", "hermes3")
        base_url = os.getenv("OLLAMA_BASE_URL", "http://localhost:11434/v1")
        return {"provider": provider, "model": model, "base_url": base_url}
    return {"provider": provider, "model": model}


@app.get("/api/stocks/search")
async def search_stocks(q: str = Query(default="")):
    """Fuzzy search through the cached A-share stock list."""
    q = q.strip().lower()
    if not q:
        return []

    results: list[dict] = []
    for stock in _stock_list:
        code = str(stock.get("code", stock.get("股票代码", "")))
        name = str(stock.get("name", stock.get("股票名称", "")))
        if q in code or q in name.lower():
            results.append({"code": code, "name": name})
            if len(results) >= 15:
                break
    return results


@app.get("/api/analyze")
async def analyze(
    request: Request,
    symbols: str = Query(..., description="Comma-separated 6-digit stock codes"),
    start_date: str = Query(..., description="Start date YYYY-MM-DD"),
    end_date: str = Query(..., description="End date YYYY-MM-DD"),
):
    """SSE endpoint — streams analysis events for one or more stocks in parallel."""
    from agents.workflow import analyze_stock
    from providers import create_provider_from_env

    symbol_list = [s.strip() for s in symbols.split(",") if s.strip()]
    if not symbol_list:
        return {"error": "No symbols provided"}

    provider = create_provider_from_env()

    async def event_generator():
        queue: asyncio.Queue[dict | None] = asyncio.Queue()
        remaining = len(symbol_list)

        async def run_one(symbol: str):
            nonlocal remaining
            try:
                async for event in analyze_stock(provider, symbol, start_date, end_date):
                    await queue.put(event)
            except Exception as e:
                await queue.put({"type": "error", "symbol": symbol, "message": str(e)})
            finally:
                remaining -= 1

        tasks = [asyncio.create_task(run_one(s)) for s in symbol_list]

        while remaining > 0 or not queue.empty():
            # Check if client disconnected
            if await request.is_disconnected():
                for t in tasks:
                    t.cancel()
                return

            try:
                event = await asyncio.wait_for(queue.get(), timeout=0.5)
                yield {"data": json.dumps(event, ensure_ascii=False)}
            except asyncio.TimeoutError:
                yield {"comment": "ping"}  # keep-alive

        await asyncio.gather(*tasks, return_exceptions=True)
        yield {"data": json.dumps({"type": "done"})}

    return EventSourceResponse(event_generator())
