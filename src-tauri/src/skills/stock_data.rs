use encoding_rs::GBK;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error as StdError;
use std::sync::OnceLock;
use std::time::Duration;

use crate::store;

// ── HTTP clients ──

static SINA_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
static EASTMONEY_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

fn sina_client() -> &'static reqwest::blocking::Client {
    SINA_CLIENT.get_or_init(|| {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::REFERER,
            "https://finance.sina.com.cn/".parse().unwrap(),
        );
        reqwest::blocking::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to build Sina HTTP client")
    })
}

fn eastmoney_client() -> &'static reqwest::blocking::Client {
    EASTMONEY_CLIENT.get_or_init(|| {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
                .parse()
                .unwrap(),
        );
        headers.insert(
            reqwest::header::REFERER,
            "https://quote.eastmoney.com/".parse().unwrap(),
        );
        reqwest::blocking::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build EastMoney HTTP client")
    })
}

fn eastmoney_get(url: &str) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let short_url = url.split('?').next().unwrap_or(url);
    debug!("[eastmoney] GET {}", url);
    let resp = eastmoney_client().get(url).send();
    match &resp {
        Ok(r) => info!("[eastmoney] {} => {}", short_url, r.status()),
        Err(e) => {
            error!("[eastmoney] {} => ERROR: {}", short_url, e);
            let mut source = e.source();
            while let Some(cause) = source {
                error!("[eastmoney]   caused by: {}", cause);
                source = cause.source();
            }
        }
    }
    resp
}

/// Fetch from Sina hq API, returns GBK-decoded UTF-8 text.
fn sina_hq_get(codes: &str) -> Option<String> {
    let url = format!("https://hq.sinajs.cn/list={}", codes);
    debug!("[sina] GET {}", url);
    let resp = sina_client().get(&url).send();
    match &resp {
        Ok(r) => info!("[sina] {} => {}", codes, r.status()),
        Err(e) => {
            error!("[sina] {} => ERROR: {}", codes, e);
            return None;
        }
    }
    let resp = resp.ok()?;
    let bytes = resp.bytes().ok()?;
    let (text, _, _) = GBK.decode(&bytes);
    Some(text.into_owned())
}

/// Parse one Sina quote line: `var hq_str_shXXXXXX="f0,f1,...";`
/// Returns fields split by comma, or None if empty/invalid.
fn parse_sina_line(line: &str) -> Option<Vec<&str>> {
    let start = line.find('"')? + 1;
    let end = line.rfind('"')?;
    if start >= end {
        return None;
    }
    let inner = &line[start..end];
    if inner.is_empty() {
        return None;
    }
    Some(inner.split(',').collect())
}

// ── Sina market prefix ──

fn sina_prefix(symbol: &str) -> &'static str {
    if symbol.starts_with('6') || symbol.starts_with('9') {
        "sh"
    } else {
        "sz"
    }
}

// ── EastMoney market code (for analysis APIs) ──

fn market_code(symbol: &str) -> u32 {
    if symbol.starts_with('6') || symbol.starts_with('9') {
        1
    } else {
        0
    }
}

fn fmt_date(d: &str) -> String {
    let d = d.replace('-', "");
    if d.len() >= 8 {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8])
    } else {
        d
    }
}

// ── Market Indices (Sina) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketIndex {
    pub code: String,
    pub name: String,
    pub price: f64,
    pub change: f64,
    pub change_pct: f64,
    pub volume: f64,
    pub turnover: f64,
    pub high: f64,
    pub low: f64,
    pub open: f64,
    pub prev_close: f64,
}

fn fetch_market_indices_sina() -> Option<Vec<MarketIndex>> {
    // Sina fields for indices:
    // 0:name 1:open 2:prev_close 3:price 4:high 5:low
    // 8:volume(手) 9:turnover(元)
    let text = sina_hq_get("sh000001,sz399001,sz399006")?;
    let codes = ["000001", "399001", "399006"];

    let mut result = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let fields = match parse_sina_line(line) {
            Some(f) if f.len() >= 10 => f,
            _ => continue,
        };
        let price: f64 = fields[3].parse().unwrap_or(0.0);
        let prev_close: f64 = fields[2].parse().unwrap_or(0.0);
        if price <= 0.0 {
            continue;
        }
        let change = price - prev_close;
        let change_pct = if prev_close > 0.0 {
            change / prev_close * 100.0
        } else {
            0.0
        };
        result.push(MarketIndex {
            code: codes.get(i).unwrap_or(&"").to_string(),
            name: fields[0].to_string(),
            price,
            change,
            change_pct,
            volume: fields[8].parse().unwrap_or(0.0),
            turnover: fields[9].parse().unwrap_or(0.0),
            high: fields[4].parse().unwrap_or(0.0),
            low: fields[5].parse().unwrap_or(0.0),
            open: fields[1].parse().unwrap_or(0.0),
            prev_close,
        });
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

pub fn get_market_indices() -> Vec<MarketIndex> {
    if let Some(data) = fetch_market_indices_sina() {
        if let Ok(json) = serde_json::to_string(&data) {
            crate::config_store::cache_set("market_indices", &json);
        }
        return data;
    }
    crate::config_store::cache_get("market_indices")
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

// ── Stock Quotes (Sina) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockQuote {
    pub code: String,
    pub name: String,
    pub price: f64,
    pub change: f64,
    pub change_pct: f64,
}

fn fetch_stock_quotes_sina(symbols: &[String]) -> Option<Vec<StockQuote>> {
    if symbols.is_empty() {
        return None;
    }
    let codes: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}{}", sina_prefix(s), s))
        .collect();
    let text = sina_hq_get(&codes.join(","))?;

    // Sina fields for stocks:
    // 0:name 1:open 2:prev_close 3:price 4:high 5:low
    let mut result = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let fields = match parse_sina_line(line) {
            Some(f) if f.len() >= 6 => f,
            _ => continue,
        };
        let price: f64 = fields[3].parse().unwrap_or(0.0);
        let prev_close: f64 = fields[2].parse().unwrap_or(0.0);
        if price <= 0.0 {
            continue;
        }
        let change = price - prev_close;
        let change_pct = if prev_close > 0.0 {
            change / prev_close * 100.0
        } else {
            0.0
        };
        if let Some(sym) = symbols.get(i) {
            result.push(StockQuote {
                code: sym.clone(),
                name: fields[0].to_string(),
                price,
                change,
                change_pct,
            });
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

pub fn get_stock_quotes(symbols: &[String]) -> Vec<StockQuote> {
    if symbols.is_empty() {
        return vec![];
    }
    let cache_key = format!("quotes:{}", symbols.join(","));
    if let Some(data) = fetch_stock_quotes_sina(symbols) {
        if let Ok(json) = serde_json::to_string(&data) {
            crate::config_store::cache_set(&cache_key, &json);
        }
        return data;
    }
    crate::config_store::cache_get(&cache_key)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

// ── Stock Name (Sina) ──

pub fn get_stock_name(symbol: &str) -> String {
    let code = format!("{}{}", sina_prefix(symbol), symbol);
    if let Some(text) = sina_hq_get(&code) {
        for line in text.lines() {
            if let Some(fields) = parse_sina_line(line) {
                if !fields.is_empty() && !fields[0].is_empty() {
                    return fields[0].to_string();
                }
            }
        }
    }
    symbol.to_string()
}

// ── Search Stocks (EastMoney — lightweight, no real-time push) ──

pub fn search_stocks(query: &str) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }

    let url = format!(
        "https://searchapi.eastmoney.com/api/suggest/get?\
         input={}&type=14&\
         token=D43BF722C8E33BDC906FB84D85E326E8&count=15",
        urlencoding::encode(query)
    );

    match eastmoney_get(&url) {
        Ok(resp) => match resp.json::<Value>() {
            Ok(body) => {
                let items = body
                    .get("QuotationCodeTable")
                    .and_then(|q| q.get("Data"))
                    .and_then(|d| d.as_array());

                match items {
                    Some(data) => data
                        .iter()
                        .filter_map(|item| {
                            let code = item.get("Code")?.as_str()?;
                            let name = item.get("Name")?.as_str()?;
                            let market_type = item.get("MktNum")?.as_str().unwrap_or("");
                            if market_type == "0" || market_type == "1" {
                                Some((code.to_string(), name.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    None => Vec::new(),
                }
            }
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

// ── Historical Price Data (EastMoney — analysis time only) ──

pub fn fetch_price_data(
    symbol: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<store::PriceRow>, String> {
    let sd = fmt_date(start_date);
    let ed = fmt_date(end_date);

    if let Some(rows) = store::load(symbol, &sd, &ed) {
        if !rows.is_empty() {
            return Ok(rows);
        }
    }

    let beg = start_date.replace('-', "");
    let end_clean = end_date.replace('-', "");
    let secid = format!("{}.{}", market_code(symbol), symbol);

    let url = format!(
        "https://push2his.eastmoney.com/api/qt/stock/kline/get?\
         secid={secid}&fields1=f1,f2,f3,f4,f5,f6&\
         fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61&\
         klt=101&fqt=1&beg={beg}&end={end_clean}&\
         ut=fa5fd1943c7b386f172d6893dbfba10b"
    );

    let resp = eastmoney_get(&url).map_err(|e| format!("HTTP error: {e}"))?;
    let body: Value = resp.json().map_err(|e| format!("JSON parse error: {e}"))?;

    let klines = body
        .get("data")
        .and_then(|d| d.get("klines"))
        .and_then(|k| k.as_array())
        .ok_or("No kline data returned")?;

    let mut rows = Vec::new();
    for kline in klines {
        let line = kline.as_str().unwrap_or("");
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 9 {
            continue;
        }
        rows.push(store::PriceRow {
            date: parts[0].to_string(),
            open: parts[1].parse().unwrap_or(0.0),
            close: parts[2].parse().unwrap_or(0.0),
            high: parts[3].parse().unwrap_or(0.0),
            low: parts[4].parse().unwrap_or(0.0),
            volume: parts[5].parse().unwrap_or(0.0),
            amount: parts[6].parse().unwrap_or(0.0),
            pct_change: parts[8].parse().unwrap_or(0.0),
        });
    }

    if !rows.is_empty() {
        store::upsert(symbol, &sd, &ed, &rows);
    }

    Ok(rows)
}

pub fn get_stock_history(symbol: &str, start_date: &str, end_date: &str) -> String {
    match fetch_price_data(symbol, start_date, end_date) {
        Ok(rows) if rows.is_empty() => format!("No price data found for {symbol}"),
        Ok(rows) => {
            let latest = &rows[rows.len() - 1];
            let high_max = rows.iter().map(|r| r.high).fold(f64::NEG_INFINITY, f64::max);
            let low_min = rows.iter().map(|r| r.low).fold(f64::INFINITY, f64::min);
            let avg_vol: f64 = rows.iter().map(|r| r.volume).sum::<f64>() / rows.len() as f64;

            let mut summary = format!(
                "Stock: {symbol}\nPeriod: {} → {}  ({} trading days)\n\
                 Latest close: {:.2}  change: {:.2}%\n\
                 Period high: {:.2}  low: {:.2}\n\
                 Avg daily volume: {:.0}\n\nRecent 20 trading days:\n\
                 {:>10} {:>8} {:>8} {:>8} {:>8} {:>12} {:>8}\n",
                rows[0].date,
                latest.date,
                rows.len(),
                latest.close,
                latest.pct_change,
                high_max,
                low_min,
                avg_vol,
                "date", "open", "close", "high", "low", "volume", "pct_chg"
            );

            let start = if rows.len() > 20 { rows.len() - 20 } else { 0 };
            for row in &rows[start..] {
                summary.push_str(&format!(
                    "{:>10} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>12.0} {:>8.2}\n",
                    row.date, row.open, row.close, row.high, row.low, row.volume, row.pct_change
                ));
            }
            summary
        }
        Err(e) => format!("Error fetching stock history for {symbol}: {e}"),
    }
}

// ── Stock Info (EastMoney — analysis time only) ──

pub fn get_stock_info(symbol: &str) -> String {
    let secid = format!("{}.{}", market_code(symbol), symbol);
    let url = format!(
        "https://push2.eastmoney.com/api/qt/stock/get?\
         secid={secid}&\
         fields=f57,f58,f43,f44,f45,f46,f47,f48,f50,f51,f52,f55,f60,f116,f117,f162,f163,f164,f167,f168,f169,f170,f171&\
         ut=fa5fd1943c7b386f172d6893dbfba10b"
    );

    match eastmoney_get(&url) {
        Ok(resp) => match resp.json::<Value>() {
            Ok(body) => {
                let d = &body["data"];
                let name = d["f58"].as_str().unwrap_or("N/A");
                let price = d["f43"].as_f64().map(|v| v / 100.0);
                let pe = d["f162"].as_f64().map(|v| v / 100.0);
                let pb = d["f167"].as_f64().map(|v| v / 100.0);
                let total_mv = d["f116"].as_f64().map(|v| v / 100_000_000.0);
                let circ_mv = d["f117"].as_f64().map(|v| v / 100_000_000.0);
                let high52 = d["f44"].as_f64().map(|v| v / 100.0);
                let low52 = d["f45"].as_f64().map(|v| v / 100.0);
                let volume = d["f47"].as_f64();
                let turnover = d["f168"].as_f64().map(|v| v / 100.0);

                format!(
                    "Stock info for {symbol} ({name}):\n\
                     最新价: {}\n\
                     市盈率(PE): {}\n\
                     市净率(PB): {}\n\
                     总市值: {}亿\n\
                     流通市值: {}亿\n\
                     52周最高: {}\n\
                     52周最低: {}\n\
                     成交量: {}\n\
                     换手率: {}%",
                    price.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                    pe.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                    pb.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                    total_mv.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                    circ_mv.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                    high52.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                    low52.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                    volume.map_or("N/A".to_string(), |v| format!("{v:.0}")),
                    turnover.map_or("N/A".to_string(), |v| format!("{v:.2}")),
                )
            }
            Err(e) => format!("Error parsing stock info for {symbol}: {e}"),
        },
        Err(e) => format!("Error fetching stock info for {symbol}: {e}"),
    }
}

// ── Financial Data (EastMoney — analysis time only) ──

pub fn get_financial_data(symbol: &str) -> String {
    let url = format!(
        "https://datacenter.eastmoney.com/securities/api/data/v1/get?\
         reportName=RPT_LICO_FN_CPD&columns=SECURITY_CODE,REPORT_DATE,BASIC_EPS,\
         WEIGHTAVG_ROE,TOTAL_OPERATE_INCOME,PARENT_NETPROFIT,TOTAL_ASSETS,\
         TOTAL_LIABILITIES&\
         filter=(SECURITY_CODE=\"{symbol}\")&pageSize=5&\
         sortTypes=-1&sortColumns=REPORT_DATE&\
         source=HSF10&client=PC"
    );

    match eastmoney_get(&url) {
        Ok(resp) => match resp.json::<Value>() {
            Ok(body) => {
                let items = body
                    .get("result")
                    .and_then(|r| r.get("data"))
                    .and_then(|d| d.as_array());

                match items {
                    Some(data) if !data.is_empty() => {
                        let mut result = format!(
                            "Annual financial data for {symbol}:\n\n{:>12} {:>12} {:>14} {:>14} {:>10} {:>10}\n",
                            "报告期", "EPS", "营业收入", "归母净利润", "ROE%", "资产负债率%"
                        );
                        for item in data {
                            let date = item["REPORT_DATE"]
                                .as_str()
                                .unwrap_or("")
                                .get(..10)
                                .unwrap_or("N/A");
                            let eps = item["BASIC_EPS"].as_f64();
                            let roe = item["WEIGHTAVG_ROE"].as_f64();
                            let revenue = item["TOTAL_OPERATE_INCOME"].as_f64();
                            let profit = item["PARENT_NETPROFIT"].as_f64();
                            let assets = item["TOTAL_ASSETS"].as_f64().unwrap_or(1.0);
                            let liabilities = item["TOTAL_LIABILITIES"].as_f64().unwrap_or(0.0);
                            let debt_ratio = liabilities / assets * 100.0;

                            result.push_str(&format!(
                                "{:>12} {:>12} {:>14} {:>14} {:>10} {:>10}\n",
                                date,
                                eps.map_or("N/A".into(), |v| format!("{v:.4}")),
                                revenue.map_or("N/A".into(), |v| format!("{:.2}亿", v / 1e8)),
                                profit.map_or("N/A".into(), |v| format!("{:.2}亿", v / 1e8)),
                                roe.map_or("N/A".into(), |v| format!("{v:.2}")),
                                format!("{debt_ratio:.2}"),
                            ));
                        }
                        result
                    }
                    _ => format!("No financial data found for {symbol}"),
                }
            }
            Err(e) => format!("Error parsing financial data for {symbol}: {e}"),
        },
        Err(e) => format!("Error fetching financial data for {symbol}: {e}"),
    }
}

// ── Stock News (EastMoney — analysis time only) ──

pub fn get_stock_news(symbol: &str, limit: usize) -> String {
    let url = format!(
        "https://search-api-web.eastmoney.com/search/jsonp?\
         cb=jQuery&param=%7B%22uid%22:%22%22,%22keyword%22:%22{symbol}%22,\
         %22type%22:[%22cmsArticleWebOld%22],%22client%22:%22web%22,\
         %22clientType%22:%22web%22,%22clientVersion%22:%22curr%22,\
         %22param%22:%7B%22cmsArticleWebOld%22:%7B%22searchScope%22:%22default%22,\
         %22sort%22:%22default%22,%22pageIndex%22:1,%22pageSize%22:{limit}%7D%7D%7D"
    );

    match eastmoney_get(&url) {
        Ok(resp) => match resp.text() {
            Ok(text) => {
                let json_start = text.find('(').map(|i| i + 1).unwrap_or(0);
                let json_end = text.rfind(')').unwrap_or(text.len());
                let json_str = &text[json_start..json_end];

                match serde_json::from_str::<Value>(json_str) {
                    Ok(data) => {
                        let articles = data
                            .get("result")
                            .and_then(|r| r.get("cmsArticleWebOld"))
                            .and_then(|c| c.as_array());

                        match articles {
                            Some(arts) if !arts.is_empty() => {
                                let mut result = format!("Recent news for {symbol}:\n\n");
                                for art in arts.iter().take(limit) {
                                    let title =
                                        art.get("title").and_then(|v| v.as_str()).unwrap_or("");
                                    let date =
                                        art.get("date").and_then(|v| v.as_str()).unwrap_or("");
                                    let content = art
                                        .get("content")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let snippet: String = content.chars().take(200).collect();
                                    result.push_str(&format!(
                                        "【{title}】\n  时间: {date}\n  摘要: {snippet}…\n\n"
                                    ));
                                }
                                result
                            }
                            _ => format!("No recent news found for {symbol}"),
                        }
                    }
                    Err(_) => format!("No recent news found for {symbol}"),
                }
            }
            Err(e) => format!("Error fetching news for {symbol}: {e}"),
        },
        Err(e) => format!("Error fetching news for {symbol}: {e}"),
    }
}
