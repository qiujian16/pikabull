use serde_json::{json, Value};

use crate::skills::stock_data;

fn sma(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    closes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i + 1 < period {
                None
            } else {
                let sum: f64 = closes[i + 1 - period..=i].iter().sum();
                Some(sum / period as f64)
            }
        })
        .collect()
}

fn ema(data: &[f64], span: usize) -> Vec<f64> {
    let k = 2.0 / (span as f64 + 1.0);
    let mut result = vec![0.0; data.len()];
    if data.is_empty() {
        return result;
    }
    result[0] = data[0];
    for i in 1..data.len() {
        result[i] = data[i] * k + result[i - 1] * (1.0 - k);
    }
    result
}

pub fn generate_stock_chart(
    symbol: &str,
    stock_name: &str,
    start_date: &str,
    end_date: &str,
) -> Option<Value> {
    let rows = stock_data::fetch_price_data(symbol, start_date, end_date).ok()?;
    if rows.is_empty() {
        return None;
    }

    let dates: Vec<&str> = rows.iter().map(|r| r.date.as_str()).collect();
    let opens: Vec<f64> = rows.iter().map(|r| r.open).collect();
    let highs: Vec<f64> = rows.iter().map(|r| r.high).collect();
    let lows: Vec<f64> = rows.iter().map(|r| r.low).collect();
    let closes: Vec<f64> = rows.iter().map(|r| r.close).collect();
    let volumes: Vec<f64> = rows.iter().map(|r| r.volume).collect();
    let pct_changes: Vec<f64> = rows.iter().map(|r| r.pct_change).collect();

    let sma20 = sma(&closes, 20);
    let sma50 = sma(&closes, 50);

    let ema12 = ema(&closes, 12);
    let ema26 = ema(&closes, 26);
    let macd_line: Vec<f64> = ema12.iter().zip(&ema26).map(|(a, b)| a - b).collect();
    let macd_signal = ema(&macd_line, 9);
    let macd_hist: Vec<f64> = macd_line
        .iter()
        .zip(&macd_signal)
        .map(|(a, b)| a - b)
        .collect();

    // RSI
    let mut rsi = vec![50.0_f64; closes.len()];
    if closes.len() >= 15 {
        let mut gains = vec![0.0; closes.len()];
        let mut losses_arr = vec![0.0; closes.len()];
        for i in 1..closes.len() {
            let d = closes[i] - closes[i - 1];
            if d > 0.0 {
                gains[i] = d;
            } else {
                losses_arr[i] = -d;
            }
        }
        let mut avg_gain: f64 = gains[1..15].iter().sum::<f64>() / 14.0;
        let mut avg_loss: f64 = losses_arr[1..15].iter().sum::<f64>() / 14.0;
        rsi[14] = if avg_loss == 0.0 {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
        };
        for i in 15..closes.len() {
            avg_gain = (avg_gain * 13.0 + gains[i]) / 14.0;
            avg_loss = (avg_loss * 13.0 + losses_arr[i]) / 14.0;
            rsi[i] = if avg_loss == 0.0 {
                100.0
            } else {
                100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
            };
        }
    }

    // Color arrays (A-share: red=up, green=down)
    let price_colors: Vec<&str> = pct_changes
        .iter()
        .map(|p| if *p >= 0.0 { "#ef5350" } else { "#26a69a" })
        .collect();
    let macd_colors: Vec<&str> = macd_hist
        .iter()
        .map(|v| if *v >= 0.0 { "#ef5350" } else { "#26a69a" })
        .collect();

    // Volume scaling
    let high_max = highs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let low_min = lows.iter().cloned().fold(f64::INFINITY, f64::min);
    let price_range = high_max - low_min;
    let vol_max = volumes.iter().cloned().fold(0.0_f64, f64::max);
    let vol_scale = if vol_max > 0.0 {
        price_range * 0.15 / vol_max
    } else {
        1.0
    };
    let scaled_volumes: Vec<f64> = volumes.iter().map(|v| v * vol_scale + low_min).collect();

    // SMA series as JSON (filter nulls)
    let sma20_y: Vec<Value> = sma20
        .iter()
        .map(|v| v.map_or(Value::Null, |x| json!(x)))
        .collect();
    let sma50_y: Vec<Value> = sma50
        .iter()
        .map(|v| v.map_or(Value::Null, |x| json!(x)))
        .collect();

    let data = json!([
        // Candlestick
        {
            "type": "candlestick",
            "x": dates,
            "open": opens,
            "high": highs,
            "low": lows,
            "close": closes,
            "name": "Price",
            "increasing": {"line": {"color": "#ef5350"}, "fillcolor": "#ef5350"},
            "decreasing": {"line": {"color": "#26a69a"}, "fillcolor": "#26a69a"},
            "xaxis": "x", "yaxis": "y"
        },
        // SMA20
        {
            "type": "scatter", "mode": "lines",
            "x": dates, "y": sma20_y,
            "name": "SMA20",
            "line": {"color": "#ff9800", "width": 1.2},
            "xaxis": "x", "yaxis": "y"
        },
        // SMA50
        {
            "type": "scatter", "mode": "lines",
            "x": dates, "y": sma50_y,
            "name": "SMA50",
            "line": {"color": "#2196f3", "width": 1.2},
            "xaxis": "x", "yaxis": "y"
        },
        // Volume
        {
            "type": "bar",
            "x": dates, "y": scaled_volumes,
            "name": "Volume",
            "marker": {"color": price_colors},
            "opacity": 0.4,
            "showlegend": false,
            "xaxis": "x", "yaxis": "y"
        },
        // MACD Hist
        {
            "type": "bar",
            "x": dates, "y": macd_hist,
            "name": "MACD Hist",
            "marker": {"color": macd_colors},
            "opacity": 0.8,
            "xaxis": "x2", "yaxis": "y2"
        },
        // MACD Line
        {
            "type": "scatter", "mode": "lines",
            "x": dates, "y": macd_line,
            "name": "MACD",
            "line": {"color": "#2196f3", "width": 1.2},
            "xaxis": "x2", "yaxis": "y2"
        },
        // MACD Signal
        {
            "type": "scatter", "mode": "lines",
            "x": dates, "y": macd_signal,
            "name": "Signal",
            "line": {"color": "#ff9800", "width": 1.2},
            "xaxis": "x2", "yaxis": "y2"
        },
        // RSI
        {
            "type": "scatter", "mode": "lines",
            "x": dates, "y": rsi,
            "name": "RSI",
            "line": {"color": "#9c27b0", "width": 1.5},
            "fill": "tozeroy",
            "fillcolor": "rgba(156,39,176,0.08)",
            "xaxis": "x3", "yaxis": "y3"
        }
    ]);

    let layout = json!({
        "autosize": true,
        "template": "plotly_white",
        "showlegend": true,
        "legend": {
            "orientation": "h", "yanchor": "bottom", "y": 1.02,
            "xanchor": "right", "x": 1, "font": {"size": 11}
        },
        "margin": {"l": 55, "r": 20, "t": 60, "b": 40},
        "plot_bgcolor": "#fafafa",
        "annotations": [
            {"text": format!("{stock_name}（{symbol}）"), "xref": "paper", "yref": "paper",
             "x": 0, "y": 1.08, "showarrow": false, "font": {"size": 14}},
            {"text": "MACD (12,26,9)", "xref": "paper", "yref": "paper",
             "x": 0, "y": 0.38, "showarrow": false, "font": {"size": 11}},
            {"text": "RSI (14)", "xref": "paper", "yref": "paper",
             "x": 0, "y": 0.17, "showarrow": false, "font": {"size": 11}}
        ],
        "xaxis": {"domain": [0, 1], "rangeslider": {"visible": false}, "anchor": "y",
                  "matches": "x3"},
        "yaxis": {"domain": [0.42, 1.0], "title": "Price (CNY)", "showgrid": true,
                  "anchor": "x"},
        "xaxis2": {"domain": [0, 1], "anchor": "y2", "matches": "x3", "showticklabels": false},
        "yaxis2": {"domain": [0.22, 0.38], "title": "MACD", "showgrid": true,
                   "anchor": "x2"},
        "xaxis3": {"domain": [0, 1], "anchor": "y3"},
        "yaxis3": {"domain": [0.0, 0.18], "title": "RSI", "showgrid": true,
                   "range": [0, 100], "anchor": "x3"},
        "shapes": [
            {"type": "line", "xref": "paper", "x0": 0, "x1": 1,
             "yref": "y3", "y0": 70, "y1": 70,
             "line": {"dash": "dot", "color": "rgba(239,83,80,0.5)"}},
            {"type": "line", "xref": "paper", "x0": 0, "x1": 1,
             "yref": "y3", "y0": 30, "y1": 30,
             "line": {"dash": "dot", "color": "rgba(38,166,154,0.5)"}}
        ]
    });

    Some(json!({"data": data, "layout": layout}))
}
