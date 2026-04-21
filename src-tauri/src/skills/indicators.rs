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

fn ema(closes: &[f64], span: usize) -> Vec<f64> {
    let k = 2.0 / (span as f64 + 1.0);
    let mut result = vec![0.0; closes.len()];
    if closes.is_empty() {
        return result;
    }
    result[0] = closes[0];
    for i in 1..closes.len() {
        result[i] = closes[i] * k + result[i - 1] * (1.0 - k);
    }
    result
}

fn rsi14(closes: &[f64]) -> Vec<Option<f64>> {
    let mut result = vec![None; closes.len()];
    if closes.len() < 15 {
        return result;
    }

    let mut gains = vec![0.0; closes.len()];
    let mut losses = vec![0.0; closes.len()];
    for i in 1..closes.len() {
        let delta = closes[i] - closes[i - 1];
        if delta > 0.0 {
            gains[i] = delta;
        } else {
            losses[i] = -delta;
        }
    }

    let mut avg_gain: f64 = gains[1..15].iter().sum::<f64>() / 14.0;
    let mut avg_loss: f64 = losses[1..15].iter().sum::<f64>() / 14.0;

    if avg_loss == 0.0 {
        result[14] = Some(100.0);
    } else {
        result[14] = Some(100.0 - 100.0 / (1.0 + avg_gain / avg_loss));
    }

    for i in 15..closes.len() {
        avg_gain = (avg_gain * 13.0 + gains[i]) / 14.0;
        avg_loss = (avg_loss * 13.0 + losses[i]) / 14.0;
        if avg_loss == 0.0 {
            result[i] = Some(100.0);
        } else {
            result[i] = Some(100.0 - 100.0 / (1.0 + avg_gain / avg_loss));
        }
    }
    result
}

fn rolling_std(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    closes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i + 1 < period {
                None
            } else {
                let slice = &closes[i + 1 - period..=i];
                let mean: f64 = slice.iter().sum::<f64>() / period as f64;
                let var: f64 = slice.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (period as f64 - 1.0);
                Some(var.sqrt())
            }
        })
        .collect()
}

pub fn get_technical_indicators(
    symbol: &str,
    start_date: &str,
    end_date: &str,
    indicator_list: &[String],
) -> String {
    let rows = match stock_data::fetch_price_data(symbol, start_date, end_date) {
        Ok(r) if r.is_empty() => return format!("No price data found for {symbol}"),
        Ok(r) => r,
        Err(e) => return format!("Error computing indicators for {symbol}: {e}"),
    };

    let closes: Vec<f64> = rows.iter().map(|r| r.close).collect();
    let dates: Vec<&str> = rows.iter().map(|r| r.date.as_str()).collect();
    let n = closes.len();

    let sma20 = if indicator_list.iter().any(|i| i == "sma20") {
        Some(sma(&closes, 20))
    } else {
        None
    };
    let sma50 = if indicator_list.iter().any(|i| i == "sma50") {
        Some(sma(&closes, 50))
    } else {
        None
    };
    let ema10 = if indicator_list.iter().any(|i| i == "ema10") {
        Some(ema(&closes, 10))
    } else {
        None
    };
    let rsi = if indicator_list.iter().any(|i| i == "rsi14") {
        Some(rsi14(&closes))
    } else {
        None
    };

    let need_macd = indicator_list
        .iter()
        .any(|i| i == "macd" || i == "macd_signal" || i == "macd_hist");
    let (macd_line, macd_signal, macd_hist) = if need_macd {
        let ema12 = ema(&closes, 12);
        let ema26 = ema(&closes, 26);
        let ml: Vec<f64> = ema12.iter().zip(&ema26).map(|(a, b)| a - b).collect();
        let ms = ema(&ml, 9);
        let mh: Vec<f64> = ml.iter().zip(&ms).map(|(a, b)| a - b).collect();
        (Some(ml), Some(ms), Some(mh))
    } else {
        (None, None, None)
    };

    let need_boll = indicator_list
        .iter()
        .any(|i| i == "boll_upper" || i == "boll_mid" || i == "boll_lower");
    let (boll_mid, boll_upper, boll_lower) = if need_boll {
        let mid = sma(&closes, 20);
        let std = rolling_std(&closes, 20);
        let upper: Vec<Option<f64>> = mid
            .iter()
            .zip(&std)
            .map(|(m, s)| match (m, s) {
                (Some(m), Some(s)) => Some(m + 2.0 * s),
                _ => None,
            })
            .collect();
        let lower: Vec<Option<f64>> = mid
            .iter()
            .zip(&std)
            .map(|(m, s)| match (m, s) {
                (Some(m), Some(s)) => Some(m - 2.0 * s),
                _ => None,
            })
            .collect();
        (Some(mid), Some(upper), Some(lower))
    } else {
        (None, None, None)
    };

    let mut header = format!("{:>10} {:>8}", "date", "close");
    for ind in indicator_list {
        header.push_str(&format!(" {:>10}", ind));
    }

    let start = if n > 30 { n - 30 } else { 0 };
    let mut result = format!(
        "Technical indicators for {symbol} ({start_date} → {end_date}):\n{header}\n"
    );

    for i in start..n {
        let mut line = format!("{:>10} {:>8.2}", dates[i], closes[i]);
        for ind in indicator_list {
            let val = match ind.as_str() {
                "sma20" => sma20.as_ref().and_then(|v| v[i]),
                "sma50" => sma50.as_ref().and_then(|v| v[i]),
                "ema10" => ema10.as_ref().map(|v| v[i]),
                "rsi14" => rsi.as_ref().and_then(|v| v[i]),
                "macd" => macd_line.as_ref().map(|v| v[i]),
                "macd_signal" => macd_signal.as_ref().map(|v| v[i]),
                "macd_hist" => macd_hist.as_ref().map(|v| v[i]),
                "boll_upper" => boll_upper.as_ref().and_then(|v| v[i]),
                "boll_mid" => boll_mid.as_ref().and_then(|v| v[i]),
                "boll_lower" => boll_lower.as_ref().and_then(|v| v[i]),
                _ => None,
            };
            match val {
                Some(v) => line.push_str(&format!(" {:>10.3}", v)),
                None => line.push_str(&format!(" {:>10}", "NaN")),
            }
        }
        result.push_str(&line);
        result.push('\n');
    }

    result
}
