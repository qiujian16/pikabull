use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use std::sync::Arc;
use crate::providers::LLMProvider;
use crate::skills::{chart, fundamental_tools, market_tools, news_tools, stock_data};

use super::base::{run_agent_stream, run_agent_streaming};

fn truncate(text: &str, chars: usize) -> String {
    if text.len() > chars {
        let mut s: String = text.chars().take(chars).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    }
}

fn detect_decision(text: &str) -> &'static str {
    let up = text.to_uppercase();
    if text.contains("买入") || up.contains("BUY") {
        "BUY"
    } else if text.contains("卖出") || up.contains("SELL") {
        "SELL"
    } else {
        "HOLD"
    }
}

fn emit(app: &AppHandle, event: Value) {
    let _ = app.emit("analysis-event", event);
}

async fn streaming_tool_agent(
    app: &AppHandle,
    provider: &dyn LLMProvider,
    symbol: &str,
    step: &str,
    label: &str,
    system: &str,
    user: &str,
    tools: &[Value],
) -> Result<String, String> {
    let (tx, mut rx) = mpsc::channel::<String>(256);

    let system = system.to_string();
    let user = user.to_string();
    let tools = tools.to_vec();

    let app_emit = app.clone();
    let symbol_owned = symbol.to_string();
    let step_owned = step.to_string();

    let reader = tokio::spawn(async move {
        let mut full_text = String::new();
        while let Some(chunk) = rx.recv().await {
            full_text.push_str(&chunk);
            emit(
                &app_emit,
                json!({"type": "chunk", "symbol": symbol_owned, "step": step_owned, "text": chunk}),
            );
        }
        full_text
    });

    let result = run_agent_streaming(provider, &system, &user, &tools, &tx).await;
    drop(tx);

    let full_text = reader.await.map_err(|e| format!("Reader error: {e}"))?;

    match result {
        Ok(_) => {
            emit(
                app,
                json!({"type": "report", "symbol": symbol, "step": step, "label": label, "content": full_text}),
            );
            Ok(full_text)
        }
        Err(e) => Err(e),
    }
}

async fn stream_agent(
    app: &AppHandle,
    provider: &dyn LLMProvider,
    symbol: &str,
    step: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let mut rx = run_agent_stream(provider, system, user).await?;
    let mut full_text = String::new();

    while let Some(chunk) = rx.recv().await {
        full_text.push_str(&chunk);
        emit(
            app,
            json!({"type": "chunk", "symbol": symbol, "step": step, "text": chunk}),
        );
    }
    Ok(full_text)
}

pub async fn analyze_stock(
    app: AppHandle,
    provider: Arc<dyn LLMProvider>,
    symbol: &str,
    start_date: &str,
    end_date: &str,
    enabled: Vec<String>,
) {
    run_analysis(app.clone(), provider, symbol, start_date, end_date, enabled).await;
    emit(&app, json!({"type": "symbol-done", "symbol": symbol}));
}

async fn run_analysis(
    app: AppHandle,
    provider: Arc<dyn LLMProvider>,
    symbol: &str,
    start_date: &str,
    end_date: &str,
    enabled: Vec<String>,
) {
    let provider: &dyn LLMProvider = provider.as_ref();
    let enabled: std::collections::HashSet<String> = enabled.into_iter().collect();

    // Resolve stock name
    let stock_name = tokio::task::spawn_blocking({
        let sym = symbol.to_string();
        move || stock_data::get_stock_name(&sym)
    })
    .await
    .unwrap_or_else(|_| symbol.to_string());

    emit(
        &app,
        json!({"type": "start", "symbol": symbol, "name": stock_name}),
    );

    // Chart generation
    emit(
        &app,
        json!({"type": "step_start", "symbol": symbol, "step": "chart", "label": "生成图表"}),
    );
    let chart_data = tokio::task::spawn_blocking({
        let sym = symbol.to_string();
        let sn = stock_name.clone();
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        move || chart::generate_stock_chart(&sym, &sn, &sd, &ed)
    })
    .await
    .unwrap_or(None);

    emit(
        &app,
        json!({"type": "chart", "symbol": symbol, "chart": chart_data}),
    );

    // Parallel: market + fundamental + news — each streams independently
    for (step, label) in [
        ("market", "技术分析"),
        ("fundamental", "基本面分析"),
        ("news", "新闻资讯"),
    ] {
        if enabled.contains(step) {
            emit(
                &app,
                json!({"type": "step_start", "symbol": symbol, "step": step, "label": label}),
            );
        }
    }

    let market_system = format!(
        "你是一位专业的A股技术分析师，正在分析 {symbol}（{stock_name}）。\n\
         使用工具获取股价历史数据和技术指标，撰写详细技术分析报告，涵盖：\n\
         1. 短中长期价格趋势\n\
         2. RSI、MACD、布林带等指标解读\n\
         3. 重要支撑位和压力位\n\
         4. 量价关系\n\
         5. 技术面综合结论\n\
         在报告末尾附 Markdown 表格，汇总关键指标数值。"
    );
    let market_user = format!(
        "请分析 {symbol}（{stock_name}），分析区间 {start_date} 至 {end_date}。\n\
         先用 get_stock_history 获取价格数据，再用 get_technical_indicators \
         计算 sma20、sma50、rsi14、macd、macd_signal、boll_upper、boll_mid、boll_lower，\
         最后撰写完整技术分析报告。"
    );
    let fundamental_system = format!(
        "你是一位专业的A股基本面分析师，正在分析 {symbol}（{stock_name}）。\n\
         使用工具获取公司信息与财务数据，撰写基本面分析报告，涵盖：\n\
         1. 公司概况（行业、主营、市值）\n\
         2. 估值（PE、PB 与行业均值对比）\n\
         3. 财务健康（营收、净利润、ROE、资产负债率趋势）\n\
         4. 成长性判断\n\
         5. 基本面综合结论与投资价值评估\n\
         在报告末尾附 Markdown 表格，汇总关键财务指标。"
    );
    let fundamental_user = format!(
        "请分析 {symbol}（{stock_name}）的基本面。\n\
         先用 get_stock_info 获取实时估值数据，再用 get_financial_data 获取近年财务摘要，\
         然后撰写完整基本面分析报告。"
    );
    let news_system = format!(
        "你是一位专业的A股资讯分析师，正在分析 {symbol}（{stock_name}）。\n\
         使用工具获取最新新闻，撰写资讯分析报告，涵盖：\n\
         1. 近期重要事件梳理\n\
         2. 公司公告与重大事项\n\
         3. 行业政策动态\n\
         4. 市场情绪判断（正面/中性/负面）\n\
         5. 新闻面对短期股价的潜在影响"
    );
    let news_user = format!(
        "请收集并分析 {symbol}（{stock_name}）的最新资讯，\
         使用 get_stock_news 获取最近 25 条新闻后撰写报告。"
    );

    let m_tools = market_tools();
    let f_tools = fundamental_tools();
    let n_tools = news_tools();

    let (market_result, fundamental_result, news_result) = tokio::join!(
        async {
            if enabled.contains("market") {
                streaming_tool_agent(
                    &app, provider, symbol, "market", "技术分析报告",
                    &market_system, &market_user, &m_tools,
                ).await
            } else {
                Ok(String::new())
            }
        },
        async {
            if enabled.contains("fundamental") {
                streaming_tool_agent(
                    &app, provider, symbol, "fundamental", "基本面分析报告",
                    &fundamental_system, &fundamental_user, &f_tools,
                ).await
            } else {
                Ok(String::new())
            }
        },
        async {
            if enabled.contains("news") {
                streaming_tool_agent(
                    &app, provider, symbol, "news", "新闻资讯报告",
                    &news_system, &news_user, &n_tools,
                ).await
            } else {
                Ok(String::new())
            }
        },
    );

    let market_report = match market_result {
        Ok(r) => r,
        Err(e) => {
            emit(
                &app,
                json!({"type": "error", "symbol": symbol, "message": format!("技术分析失败: {e}")}),
            );
            return;
        }
    };
    let fundamental_report = match fundamental_result {
        Ok(r) => r,
        Err(e) => {
            emit(
                &app,
                json!({"type": "error", "symbol": symbol, "message": format!("基本面分析失败: {e}")}),
            );
            return;
        }
    };
    let news_report = match news_result {
        Ok(r) => r,
        Err(e) => {
            emit(
                &app,
                json!({"type": "error", "symbol": symbol, "message": format!("新闻分析失败: {e}")}),
            );
            return;
        }
    };

    // Bull researcher
    let bull_arg = if enabled.contains("bull") {
        emit(
            &app,
            json!({"type": "step_start", "symbol": symbol, "step": "bull", "label": "🐂 多方论据"}),
        );
        let bull_system = format!(
            "你是持乐观立场的A股研究员（多方），正在为 {symbol}（{stock_name}）构建买入论据。\n\
             仅从积极角度论证，不需要平衡陈述，尽可能有力地支持买入决策。"
        );
        let bull_user = format!(
            "基于以下分析报告，请构建 {symbol}（{stock_name}）的最强买入论点：\n\n\
             **技术分析：**\n{}\n\n\
             **基本面分析：**\n{}\n\n\
             **资讯分析：**\n{}",
            truncate(&market_report, 1200),
            truncate(&fundamental_report, 1200),
            truncate(&news_report, 1200),
        );
        let r =
            match stream_agent(&app, provider, symbol, "bull", &bull_system, &bull_user).await {
                Ok(r) => r,
                Err(e) => {
                    emit(
                        &app,
                        json!({"type": "error", "symbol": symbol, "message": format!("多方分析失败: {e}")}),
                    );
                    return;
                }
            };
        emit(
            &app,
            json!({"type": "report", "symbol": symbol, "step": "bull", "label": "多方论据", "content": r}),
        );
        r
    } else {
        String::new()
    };

    // Bear researcher
    let bear_arg = if enabled.contains("bear") {
        emit(
            &app,
            json!({"type": "step_start", "symbol": symbol, "step": "bear", "label": "🐻 空方论据"}),
        );
        let bear_system = format!(
            "你是持悲观立场的A股研究员（空方），正在为回避/卖出 {symbol}（{stock_name}）构建论据。\n\
             仅从风险与负面角度论证，不需要平衡陈述，尽可能有力地支持卖出/回避决策。"
        );
        let bear_user = format!(
            "基于以下分析报告，请构建 {symbol}（{stock_name}）的最强卖出/回避论点：\n\n\
             **技术分析：**\n{}\n\n\
             **基本面分析：**\n{}\n\n\
             **资讯分析：**\n{}",
            truncate(&market_report, 1200),
            truncate(&fundamental_report, 1200),
            truncate(&news_report, 1200),
        );
        let r =
            match stream_agent(&app, provider, symbol, "bear", &bear_system, &bear_user).await {
                Ok(r) => r,
                Err(e) => {
                    emit(
                        &app,
                        json!({"type": "error", "symbol": symbol, "message": format!("空方分析失败: {e}")}),
                    );
                    return;
                }
            };
        emit(
            &app,
            json!({"type": "report", "symbol": symbol, "step": "bear", "label": "空方论据", "content": r}),
        );
        r
    } else {
        String::new()
    };

    // Trader decision
    let trade_plan = if enabled.contains("trader") {
        emit(
            &app,
            json!({"type": "step_start", "symbol": symbol, "step": "trader", "label": "交易决策"}),
        );
        let trader_system = format!(
            "你是一位经验丰富的A股基金经理，综合所有分析为 {symbol}（{stock_name}）给出明确投资建议。\n\
             建议必须包含：\n\
             1. 操作指令（开头加粗标注）：**买入(BUY)** / **持有(HOLD)** / **卖出(SELL)**\n\
             2. 目标价或止损价\n\
             3. 建议仓位（0-30%）\n\
             4. 综合投资理由（技术面 + 基本面 + 资讯面）\n\
             5. 主要风险提示"
        );
        let trader_user = format!(
            "股票：{symbol}（{stock_name}）\n\n\
             **技术分析摘要：**\n{}\n\n\
             **基本面摘要：**\n{}\n\n\
             **资讯摘要：**\n{}\n\n\
             **多方论点：**\n{}\n\n\
             **空方论点：**\n{}\n\n\
             请给出明确投资决策。",
            truncate(&market_report, 800),
            truncate(&fundamental_report, 800),
            truncate(&news_report, 400),
            truncate(&bull_arg, 600),
            truncate(&bear_arg, 600),
        );
        let r = match stream_agent(&app, provider, symbol, "trader", &trader_system, &trader_user)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                emit(
                    &app,
                    json!({"type": "error", "symbol": symbol, "message": format!("交易决策失败: {e}")}),
                );
                return;
            }
        };
        emit(
            &app,
            json!({"type": "report", "symbol": symbol, "step": "trader", "label": "交易决策", "content": r}),
        );
        r
    } else {
        String::new()
    };

    // Risk manager
    if enabled.contains("risk") {
        emit(
            &app,
            json!({"type": "step_start", "symbol": symbol, "step": "risk", "label": "风险评估"}),
        );
        let risk_system = format!(
            "你是一位A股风险管理专家，对基金经理的投资决策进行风险评估。\n\
             评估内容：\n\
             1. 主要风险因素（市场风险、个股风险、流动性风险、政策风险）\n\
             2. 风险量化（高/中/低）\n\
             3. 建议止损位和止盈位\n\
             4. 最终建议：维持或调整基金经理决策\n\
             5. 综合风险评级（低/中/高）"
        );
        let risk_user = format!(
            "股票：{symbol}（{stock_name}）\n\n\
             **基金经理决策：**\n{trade_plan}\n\n\
             请对上述投资决策进行完整风险评估，并给出最终结论。"
        );
        let risk_assessment =
            match stream_agent(&app, provider, symbol, "risk", &risk_system, &risk_user).await {
                Ok(r) => r,
                Err(e) => {
                    emit(
                        &app,
                        json!({"type": "error", "symbol": symbol, "message": format!("风险评估失败: {e}")}),
                    );
                    return;
                }
            };
        emit(
            &app,
            json!({"type": "report", "symbol": symbol, "step": "risk", "label": "风险评估", "content": risk_assessment}),
        );
    }

    // Final decision
    emit(
        &app,
        json!({
            "type": "final",
            "symbol": symbol,
            "name": stock_name,
            "decision": detect_decision(&trade_plan),
        }),
    );
}
