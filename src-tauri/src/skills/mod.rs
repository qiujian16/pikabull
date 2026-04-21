pub mod chart;
pub mod indicators;
pub mod stock_data;

use serde_json::{json, Value};

pub fn market_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "get_stock_history",
                "description": "Fetch historical daily OHLCV data for a China A-share stock. Returns date, open, close, high, low, volume, pct_change for the requested period.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "6-digit A-share code, e.g. '000001' (Ping An Bank), '600519' (Moutai)" },
                        "start_date": { "type": "string", "description": "Start date YYYYMMDD, e.g. '20240101'" },
                        "end_date": { "type": "string", "description": "End date YYYYMMDD, e.g. '20241231'" }
                    },
                    "required": ["symbol", "start_date", "end_date"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_technical_indicators",
                "description": "Calculate technical indicators from price data. Available: sma20, sma50, ema10, rsi14, macd, macd_signal, macd_hist, boll_upper, boll_mid, boll_lower.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "6-digit A-share code" },
                        "start_date": { "type": "string", "description": "Start date YYYYMMDD" },
                        "end_date": { "type": "string", "description": "End date YYYYMMDD" },
                        "indicators": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Indicator names: sma20, sma50, ema10, rsi14, macd, macd_signal, macd_hist, boll_upper, boll_mid, boll_lower"
                        }
                    },
                    "required": ["symbol", "start_date", "end_date", "indicators"]
                }
            }
        }),
    ]
}

pub fn fundamental_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "get_stock_info",
                "description": "Get basic real-time information for an A-share stock: PE, PB, market cap, industry, 52-week high/low, etc.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "6-digit A-share code" }
                    },
                    "required": ["symbol"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_financial_data",
                "description": "Get annual financial summary for an A-share company: revenue, net profit, ROE, debt ratio, EPS from recent annual reports.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "6-digit A-share code" }
                    },
                    "required": ["symbol"]
                }
            }
        }),
    ]
}

pub fn news_tools() -> Vec<Value> {
    vec![json!({
        "type": "function",
        "function": {
            "name": "get_stock_news",
            "description": "Fetch recent news articles and announcements for an A-share stock.",
            "parameters": {
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "6-digit A-share code" },
                    "limit": { "type": "integer", "description": "Max number of articles to return (default 20)" }
                },
                "required": ["symbol"]
            }
        }
    })]
}

pub fn execute_tool(tool_name: &str, tool_input: &Value) -> String {
    match tool_name {
        "get_stock_history" => {
            let symbol = tool_input
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let start = tool_input
                .get("start_date")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let end = tool_input
                .get("end_date")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            stock_data::get_stock_history(symbol, start, end)
        }
        "get_technical_indicators" => {
            let symbol = tool_input
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let start = tool_input
                .get("start_date")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let end = tool_input
                .get("end_date")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let indicator_list: Vec<String> = tool_input
                .get("indicators")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            indicators::get_technical_indicators(symbol, start, end, &indicator_list)
        }
        "get_stock_info" => {
            let symbol = tool_input
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            stock_data::get_stock_info(symbol)
        }
        "get_financial_data" => {
            let symbol = tool_input
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            stock_data::get_financial_data(symbol)
        }
        "get_stock_news" => {
            let symbol = tool_input
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = tool_input
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as usize;
            stock_data::get_stock_news(symbol, limit)
        }
        _ => format!("Unknown tool: {tool_name}"),
    }
}
