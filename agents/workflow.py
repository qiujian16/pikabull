"""Multi-agent stock analysis workflow.

Pipeline (mirrors TradingAgents but uses pure Claude/OpenAI API tool use
instead of LangChain/LangGraph):

  1. Chart generation          (akshare → Plotly, no LLM)
  2. Market Analyst            (technical analysis, uses tools)
  3. Fundamental Analyst       (financials, uses tools)
  4. News Analyst              (news sentiment, uses tools)
     ── steps 2-4 run in parallel ──
  5. Bull Researcher           (pro-buy argument, no tools)
  6. Bear Researcher           (pro-sell argument, no tools)
     ── steps 5-6 run in parallel ──
  7. Trader                    (investment decision, no tools)
  8. Risk Manager              (risk assessment & final call, no tools)

Each step yields an SSE event dict for the frontend.
"""
from __future__ import annotations

import asyncio
from typing import AsyncGenerator

import akshare as ak

from providers.base import BaseLLMProvider
from skills.akshare_tools import MARKET_TOOLS, FUNDAMENTAL_TOOLS, NEWS_TOOLS
from skills.chart_tools import generate_stock_chart
from .base_agent import run_agent, run_agent_stream


# ── Helpers ────────────────────────────────────────────────────────────────────

async def _get_stock_name(symbol: str) -> str:
    """Best-effort: resolve 6-digit code to Chinese company name."""
    loop = asyncio.get_event_loop()
    try:
        df = await loop.run_in_executor(None, ak.stock_individual_info_em, symbol)
        row = df[df.iloc[:, 0] == "股票简称"]
        if not row.empty:
            return str(row.iloc[0, 1])
    except Exception:
        pass
    return symbol


def _truncate(text: str, chars: int = 1200) -> str:
    return text[:chars] + "…" if len(text) > chars else text


def _detect_decision(text: str) -> str:
    up = text.upper()
    if "买入" in text or "BUY" in up:
        return "BUY"
    if "卖出" in text or "SELL" in up:
        return "SELL"
    return "HOLD"


# ── Individual analysts ────────────────────────────────────────────────────────

async def _market_analyst(
    provider: BaseLLMProvider,
    symbol: str,
    stock_name: str,
    start_date: str,
    end_date: str,
) -> str:
    return await run_agent(
        provider=provider,
        system_prompt=(
            f"你是一位专业的A股技术分析师，正在分析 {symbol}（{stock_name}）。\n"
            "使用工具获取股价历史数据和技术指标，撰写详细技术分析报告，涵盖：\n"
            "1. 短中长期价格趋势\n"
            "2. RSI、MACD、布林带等指标解读\n"
            "3. 重要支撑位和压力位\n"
            "4. 量价关系\n"
            "5. 技术面综合结论\n"
            "在报告末尾附 Markdown 表格，汇总关键指标数值。"
        ),
        user_message=(
            f"请分析 {symbol}（{stock_name}），分析区间 {start_date} 至 {end_date}。\n"
            "先用 get_stock_history 获取价格数据，再用 get_technical_indicators "
            "计算 sma20、sma50、rsi14、macd、macd_signal、boll_upper、boll_mid、boll_lower，"
            "最后撰写完整技术分析报告。"
        ),
        tools=MARKET_TOOLS,
    )


async def _fundamental_analyst(
    provider: BaseLLMProvider,
    symbol: str,
    stock_name: str,
) -> str:
    return await run_agent(
        provider=provider,
        system_prompt=(
            f"你是一位专业的A股基本面分析师，正在分析 {symbol}（{stock_name}）。\n"
            "使用工具获取公司信息与财务数据，撰写基本面分析报告，涵盖：\n"
            "1. 公司概况（行业、主营、市值）\n"
            "2. 估值（PE、PB 与行业均值对比）\n"
            "3. 财务健康（营收、净利润、ROE、资产负债率趋势）\n"
            "4. 成长性判断\n"
            "5. 基本面综合结论与投资价值评估\n"
            "在报告末尾附 Markdown 表格，汇总关键财务指标。"
        ),
        user_message=(
            f"请分析 {symbol}（{stock_name}）的基本面。\n"
            "先用 get_stock_info 获取实时估值数据，再用 get_financial_data 获取近年财务摘要，"
            "然后撰写完整基本面分析报告。"
        ),
        tools=FUNDAMENTAL_TOOLS,
    )


async def _news_analyst(
    provider: BaseLLMProvider,
    symbol: str,
    stock_name: str,
) -> str:
    return await run_agent(
        provider=provider,
        system_prompt=(
            f"你是一位专业的A股资讯分析师，正在分析 {symbol}（{stock_name}）。\n"
            "使用工具获取最新新闻，撰写资讯分析报告，涵盖：\n"
            "1. 近期重要事件梳理\n"
            "2. 公司公告与重大事项\n"
            "3. 行业政策动态\n"
            "4. 市场情绪判断（正面/中性/负面）\n"
            "5. 新闻面对短期股价的潜在影响"
        ),
        user_message=(
            f"请收集并分析 {symbol}（{stock_name}）的最新资讯，"
            "使用 get_stock_news 获取最近 25 条新闻后撰写报告。"
        ),
        tools=NEWS_TOOLS,
    )


def _bull_prompts(symbol: str, stock_name: str, market_report: str,
                  fundamental_report: str, news_report: str) -> tuple[str, str]:
    system = (
        f"你是持乐观立场的A股研究员（多方），正在为 {symbol}（{stock_name}）构建买入论据。\n"
        "仅从积极角度论证，不需要平衡陈述，尽可能有力地支持买入决策。"
    )
    user = (
        f"基于以下分析报告，请构建 {symbol}（{stock_name}）的最强买入论点：\n\n"
        f"**技术分析：**\n{_truncate(market_report)}\n\n"
        f"**基本面分析：**\n{_truncate(fundamental_report)}\n\n"
        f"**资讯分析：**\n{_truncate(news_report)}"
    )
    return system, user


def _bear_prompts(symbol: str, stock_name: str, market_report: str,
                  fundamental_report: str, news_report: str) -> tuple[str, str]:
    system = (
        f"你是持悲观立场的A股研究员（空方），正在为回避/卖出 {symbol}（{stock_name}）构建论据。\n"
        "仅从风险与负面角度论证，不需要平衡陈述，尽可能有力地支持卖出/回避决策。"
    )
    user = (
        f"基于以下分析报告，请构建 {symbol}（{stock_name}）的最强卖出/回避论点：\n\n"
        f"**技术分析：**\n{_truncate(market_report)}\n\n"
        f"**基本面分析：**\n{_truncate(fundamental_report)}\n\n"
        f"**资讯分析：**\n{_truncate(news_report)}"
    )
    return system, user


def _trader_prompts(symbol: str, stock_name: str, market_report: str,
                    fundamental_report: str, news_report: str,
                    bull_arg: str, bear_arg: str) -> tuple[str, str]:
    system = (
        f"你是一位经验丰富的A股基金经理，综合所有分析为 {symbol}（{stock_name}）给出明确投资建议。\n"
        "建议必须包含：\n"
        "1. 操作指令（开头加粗标注）：**买入(BUY)** / **持有(HOLD)** / **卖出(SELL)**\n"
        "2. 目标价或止损价\n"
        "3. 建议仓位（0-30%）\n"
        "4. 综合投资理由（技术面 + 基本面 + 资讯面）\n"
        "5. 主要风险提示"
    )
    user = (
        f"股票：{symbol}（{stock_name}）\n\n"
        f"**技术分析摘要：**\n{_truncate(market_report, 800)}\n\n"
        f"**基本面摘要：**\n{_truncate(fundamental_report, 800)}\n\n"
        f"**资讯摘要：**\n{_truncate(news_report, 400)}\n\n"
        f"**多方论点：**\n{_truncate(bull_arg, 600)}\n\n"
        f"**空方论点：**\n{_truncate(bear_arg, 600)}\n\n"
        "请给出明确投资决策。"
    )
    return system, user


def _risk_prompts(symbol: str, stock_name: str, trade_plan: str) -> tuple[str, str]:
    system = (
        f"你是一位A股风险管理专家，对基金经理的投资决策进行风险评估。\n"
        "评估内容：\n"
        "1. 主要风险因素（市场风险、个股风险、流动性风险、政策风险）\n"
        "2. 风险量化（高/中/低）\n"
        "3. 建议止损位和止盈位\n"
        "4. 最终建议：维持或调整基金经理决策\n"
        "5. 综合风险评级（低/中/高）"
    )
    user = (
        f"股票：{symbol}（{stock_name}）\n\n"
        f"**基金经理决策：**\n{trade_plan}\n\n"
        "请对上述投资决策进行完整风险评估，并给出最终结论。"
    )
    return system, user


# ── Main orchestrator ──────────────────────────────────────────────────────────

async def analyze_stock(
    provider: BaseLLMProvider,
    symbol: str,
    start_date: str,
    end_date: str,
) -> AsyncGenerator[dict, None]:
    """
    Full pipeline for one stock. Yields SSE event dicts at each step.

    Event types:
      start        – analysis beginning, includes stock name
      step_start   – a step is now running
      chart        – Plotly figure JSON
      chunk        – streaming text fragment {step, text}
      report       – complete report for a step (finalises any streaming section)
      final        – trade plan + risk assessment + BUY/HOLD/SELL decision
      error        – something went wrong
    """
    loop = asyncio.get_event_loop()

    # ── Resolve name ───────────────────────────────────────────────────────────
    stock_name = await _get_stock_name(symbol)
    yield {"type": "start", "symbol": symbol, "name": stock_name}

    # ── Chart ──────────────────────────────────────────────────────────────────
    yield {"type": "step_start", "symbol": symbol, "step": "chart", "label": "生成图表"}
    chart_data = await loop.run_in_executor(
        None, generate_stock_chart, symbol, stock_name, start_date, end_date
    )
    yield {"type": "chart", "symbol": symbol, "chart": chart_data}

    # ── Parallel: market + fundamental + news analysts ─────────────────────────
    for step, label in [
        ("market", "技术分析"),
        ("fundamental", "基本面分析"),
        ("news", "新闻资讯"),
    ]:
        yield {"type": "step_start", "symbol": symbol, "step": step, "label": label}

    try:
        market_report, fundamental_report, news_report = await asyncio.gather(
            _market_analyst(provider, symbol, stock_name, start_date, end_date),
            _fundamental_analyst(provider, symbol, stock_name),
            _news_analyst(provider, symbol, stock_name),
        )
    except Exception as e:
        yield {"type": "error", "symbol": symbol, "message": f"Analysis failed: {e}"}
        return

    yield {
        "type": "report", "symbol": symbol, "step": "market",
        "label": "技术分析报告", "content": market_report,
    }
    yield {
        "type": "report", "symbol": symbol, "step": "fundamental",
        "label": "基本面分析报告", "content": fundamental_report,
    }
    yield {
        "type": "report", "symbol": symbol, "step": "news",
        "label": "新闻资讯报告", "content": news_report,
    }

    # ── Waterfall: bull researcher first, then bear ────────────────────────────
    yield {"type": "step_start", "symbol": symbol, "step": "bull", "label": "🐂 多方论据"}
    bull_arg = ""
    bull_sys, bull_user = _bull_prompts(
        symbol, stock_name, market_report, fundamental_report, news_report
    )
    try:
        async for chunk in run_agent_stream(provider, bull_sys, bull_user):
            bull_arg += chunk
            yield {"type": "chunk", "symbol": symbol, "step": "bull", "text": chunk}
    except Exception as e:
        yield {"type": "error", "symbol": symbol, "message": f"Bull researcher failed: {e}"}
        return
    yield {"type": "report", "symbol": symbol, "step": "bull", "label": "多方论据", "content": bull_arg}

    yield {"type": "step_start", "symbol": symbol, "step": "bear", "label": "🐻 空方论据"}
    bear_arg = ""
    bear_sys, bear_user = _bear_prompts(
        symbol, stock_name, market_report, fundamental_report, news_report
    )
    try:
        async for chunk in run_agent_stream(provider, bear_sys, bear_user):
            bear_arg += chunk
            yield {"type": "chunk", "symbol": symbol, "step": "bear", "text": chunk}
    except Exception as e:
        yield {"type": "error", "symbol": symbol, "message": f"Bear researcher failed: {e}"}
        return
    yield {"type": "report", "symbol": symbol, "step": "bear", "label": "空方论据", "content": bear_arg}

    # ── Streaming: Trader ──────────────────────────────────────────────────────
    yield {"type": "step_start", "symbol": symbol, "step": "trader", "label": "交易决策"}
    trade_plan = ""
    try:
        trader_sys, trader_user = _trader_prompts(
            symbol, stock_name, market_report, fundamental_report, news_report,
            bull_arg, bear_arg,
        )
        async for chunk in run_agent_stream(provider, trader_sys, trader_user):
            trade_plan += chunk
            yield {"type": "chunk", "symbol": symbol, "step": "trader", "text": chunk}
    except Exception as e:
        yield {"type": "error", "symbol": symbol, "message": f"Trader failed: {e}"}
        return
    yield {
        "type": "report", "symbol": symbol, "step": "trader",
        "label": "交易决策", "content": trade_plan,
    }

    # ── Streaming: Risk Manager ────────────────────────────────────────────────
    yield {"type": "step_start", "symbol": symbol, "step": "risk", "label": "风险评估"}
    risk_assessment = ""
    try:
        risk_sys, risk_user = _risk_prompts(symbol, stock_name, trade_plan)
        async for chunk in run_agent_stream(provider, risk_sys, risk_user):
            risk_assessment += chunk
            yield {"type": "chunk", "symbol": symbol, "step": "risk", "text": chunk}
    except Exception as e:
        yield {"type": "error", "symbol": symbol, "message": f"Risk manager failed: {e}"}
        return
    yield {
        "type": "report", "symbol": symbol, "step": "risk",
        "label": "风险评估", "content": risk_assessment,
    }

    yield {
        "type": "final",
        "symbol": symbol,
        "name": stock_name,
        "decision": _detect_decision(trade_plan),
        "trade_plan": trade_plan,
        "risk_assessment": risk_assessment,
    }
