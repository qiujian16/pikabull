use super::metrics::{self, BacktestMetrics};
use super::strategy::*;
use crate::skills::indicators::{ema, rolling_std, rsi, sma};
use crate::store::PriceRow;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestConfig {
    pub symbol: String,
    pub start_date: String,
    pub end_date: String,
    pub initial_capital: f64,
    pub strategy: Strategy,
    pub commission_rate: f64,
    pub stamp_tax_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestResult {
    pub config: BacktestConfig,
    pub metrics: BacktestMetrics,
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<EquityPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    pub entry_date: String,
    pub entry_price: f64,
    pub exit_date: String,
    pub exit_price: f64,
    pub shares: u32,
    pub pnl: f64,
    pub pnl_pct: f64,
    pub holding_days: u32,
    pub exit_reason: ExitReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    Signal,
    StopLoss,
    TakeProfit,
    TrailingStop,
    EndOfPeriod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EquityPoint {
    pub date: String,
    pub portfolio_value: f64,
    pub benchmark_value: f64,
    pub drawdown_pct: f64,
}

struct IndicatorCache {
    closes: Vec<f64>,
    highs: Vec<f64>,
    lows: Vec<f64>,
    volumes: Vec<f64>,
    sma_cache: std::collections::HashMap<usize, Vec<Option<f64>>>,
    ema_cache: std::collections::HashMap<usize, Vec<f64>>,
    rsi_cache: std::collections::HashMap<usize, Vec<Option<f64>>>,
    macd_line: Vec<f64>,
    macd_signal: Vec<f64>,
    macd_hist: Vec<f64>,
    boll_cache: std::collections::HashMap<(usize, u64), (Vec<Option<f64>>, Vec<Option<f64>>, Vec<Option<f64>>)>,
    volume_sma_cache: std::collections::HashMap<usize, Vec<Option<f64>>>,
}

impl IndicatorCache {
    fn new(rows: &[PriceRow], strategy: &Strategy) -> Self {
        let closes: Vec<f64> = rows.iter().map(|r| r.close).collect();
        let highs: Vec<f64> = rows.iter().map(|r| r.high).collect();
        let lows: Vec<f64> = rows.iter().map(|r| r.low).collect();
        let volumes: Vec<f64> = rows.iter().map(|r| r.volume).collect();

        let mut needed_sma: HashSet<usize> = HashSet::new();
        let mut needed_ema: HashSet<usize> = HashSet::new();
        let mut needed_rsi: HashSet<usize> = HashSet::new();
        let mut need_macd = false;
        let mut needed_boll: HashSet<(usize, u64)> = HashSet::new();
        let mut needed_vol_sma: HashSet<usize> = HashSet::new();

        collect_needed_indicators(
            &strategy.entry,
            &mut needed_sma,
            &mut needed_ema,
            &mut needed_rsi,
            &mut need_macd,
            &mut needed_boll,
            &mut needed_vol_sma,
        );
        collect_needed_indicators(
            &strategy.exit,
            &mut needed_sma,
            &mut needed_ema,
            &mut needed_rsi,
            &mut need_macd,
            &mut needed_boll,
            &mut needed_vol_sma,
        );

        let mut sma_cache = std::collections::HashMap::new();
        for &period in &needed_sma {
            sma_cache.insert(period, sma(&closes, period));
        }

        let mut ema_cache = std::collections::HashMap::new();
        for &period in &needed_ema {
            ema_cache.insert(period, ema(&closes, period));
        }

        let mut rsi_cache = std::collections::HashMap::new();
        for &period in &needed_rsi {
            rsi_cache.insert(period, rsi(&closes, period));
        }

        let (macd_line, macd_signal, macd_hist) = if need_macd {
            let ema12 = ema(&closes, 12);
            let ema26 = ema(&closes, 26);
            let ml: Vec<f64> = ema12.iter().zip(&ema26).map(|(a, b)| a - b).collect();
            let ms = ema(&ml, 9);
            let mh: Vec<f64> = ml.iter().zip(&ms).map(|(a, b)| a - b).collect();
            (ml, ms, mh)
        } else {
            (vec![], vec![], vec![])
        };

        let mut boll_cache = std::collections::HashMap::new();
        for &(period, num_std_bits) in &needed_boll {
            let num_std = f64::from_bits(num_std_bits);
            let mid = sma(&closes, period);
            let std_vals = rolling_std(&closes, period);
            let upper: Vec<Option<f64>> = mid
                .iter()
                .zip(&std_vals)
                .map(|(m, s)| match (m, s) {
                    (Some(m), Some(s)) => Some(m + num_std * s),
                    _ => None,
                })
                .collect();
            let lower: Vec<Option<f64>> = mid
                .iter()
                .zip(&std_vals)
                .map(|(m, s)| match (m, s) {
                    (Some(m), Some(s)) => Some(m - num_std * s),
                    _ => None,
                })
                .collect();
            boll_cache.insert((period, num_std_bits), (mid, upper, lower));
        }

        let mut volume_sma_cache = std::collections::HashMap::new();
        for &period in &needed_vol_sma {
            volume_sma_cache.insert(period, sma(&volumes, period));
        }

        IndicatorCache {
            closes,
            highs,
            lows,
            volumes,
            sma_cache,
            ema_cache,
            rsi_cache,
            macd_line,
            macd_signal,
            macd_hist,
            boll_cache,
            volume_sma_cache,
        }
    }
}

fn collect_needed_indicators(
    group: &ConditionGroup,
    needed_sma: &mut HashSet<usize>,
    needed_ema: &mut HashSet<usize>,
    needed_rsi: &mut HashSet<usize>,
    need_macd: &mut bool,
    needed_boll: &mut HashSet<(usize, u64)>,
    needed_vol_sma: &mut HashSet<usize>,
) {
    for cond in &group.conditions {
        match cond {
            Condition::Indicator(ic) => match ic {
                IndicatorCondition::RsiAbove { period, .. }
                | IndicatorCondition::RsiBelow { period, .. }
                | IndicatorCondition::RsiCrossesAbove { period, .. }
                | IndicatorCondition::RsiCrossesBelow { period, .. } => {
                    needed_rsi.insert(*period);
                }
                IndicatorCondition::SmaCrossesAboveSma {
                    fast_period,
                    slow_period,
                }
                | IndicatorCondition::SmaCrossesBelowSma {
                    fast_period,
                    slow_period,
                } => {
                    needed_sma.insert(*fast_period);
                    needed_sma.insert(*slow_period);
                }
                IndicatorCondition::EmaCrossesAboveEma {
                    fast_period,
                    slow_period,
                }
                | IndicatorCondition::EmaCrossesBelowEma {
                    fast_period,
                    slow_period,
                } => {
                    needed_ema.insert(*fast_period);
                    needed_ema.insert(*slow_period);
                }
                IndicatorCondition::PriceAboveSma { period }
                | IndicatorCondition::PriceBelowSma { period } => {
                    needed_sma.insert(*period);
                }
                IndicatorCondition::PriceAboveEma { period }
                | IndicatorCondition::PriceBelowEma { period } => {
                    needed_ema.insert(*period);
                }
                IndicatorCondition::MacdCrossesAboveSignal
                | IndicatorCondition::MacdCrossesBelowSignal
                | IndicatorCondition::MacdHistogramPositive
                | IndicatorCondition::MacdHistogramNegative => {
                    *need_macd = true;
                }
                IndicatorCondition::PriceBelowLowerBoll { period, num_std }
                | IndicatorCondition::PriceAboveUpperBoll { period, num_std }
                | IndicatorCondition::PriceCrossesAboveLowerBoll { period, num_std }
                | IndicatorCondition::PriceCrossesBelowUpperBoll { period, num_std } => {
                    needed_boll.insert((*period, num_std.to_bits()));
                }
                IndicatorCondition::PriceAbove { .. } | IndicatorCondition::PriceBelow { .. } => {}
                IndicatorCondition::VolumeAboveAvg { period, .. } => {
                    needed_vol_sma.insert(*period);
                }
            },
            Condition::Group(g) => {
                collect_needed_indicators(
                    g,
                    needed_sma,
                    needed_ema,
                    needed_rsi,
                    need_macd,
                    needed_boll,
                    needed_vol_sma,
                );
            }
        }
    }
}

fn evaluate_condition(cond: &Condition, i: usize, cache: &IndicatorCache) -> bool {
    match cond {
        Condition::Indicator(ic) => evaluate_indicator(ic, i, cache),
        Condition::Group(g) => evaluate_group(g, i, cache),
    }
}

fn evaluate_group(group: &ConditionGroup, i: usize, cache: &IndicatorCache) -> bool {
    if group.conditions.is_empty() {
        return false;
    }
    match group.logic {
        Logic::And => group
            .conditions
            .iter()
            .all(|c| evaluate_condition(c, i, cache)),
        Logic::Or => group
            .conditions
            .iter()
            .any(|c| evaluate_condition(c, i, cache)),
    }
}

fn evaluate_indicator(ic: &IndicatorCondition, i: usize, cache: &IndicatorCache) -> bool {
    match ic {
        IndicatorCondition::RsiAbove { period, threshold } => cache
            .rsi_cache
            .get(period)
            .and_then(|v| v[i])
            .map_or(false, |v| v > *threshold),

        IndicatorCondition::RsiBelow { period, threshold } => cache
            .rsi_cache
            .get(period)
            .and_then(|v| v[i])
            .map_or(false, |v| v < *threshold),

        IndicatorCondition::RsiCrossesAbove { period, threshold } => {
            if i == 0 {
                return false;
            }
            let vals = match cache.rsi_cache.get(period) {
                Some(v) => v,
                None => return false,
            };
            let prev = vals[i - 1];
            let curr = vals[i];
            matches!((prev, curr), (Some(p), Some(c)) if p <= *threshold && c > *threshold)
        }

        IndicatorCondition::RsiCrossesBelow { period, threshold } => {
            if i == 0 {
                return false;
            }
            let vals = match cache.rsi_cache.get(period) {
                Some(v) => v,
                None => return false,
            };
            let prev = vals[i - 1];
            let curr = vals[i];
            matches!((prev, curr), (Some(p), Some(c)) if p >= *threshold && c < *threshold)
        }

        IndicatorCondition::SmaCrossesAboveSma {
            fast_period,
            slow_period,
        } => {
            if i == 0 {
                return false;
            }
            let fast = match cache.sma_cache.get(fast_period) {
                Some(v) => v,
                None => return false,
            };
            let slow = match cache.sma_cache.get(slow_period) {
                Some(v) => v,
                None => return false,
            };
            match (fast[i - 1], fast[i], slow[i - 1], slow[i]) {
                (Some(fp), Some(fc), Some(sp), Some(sc)) => fp <= sp && fc > sc,
                _ => false,
            }
        }

        IndicatorCondition::SmaCrossesBelowSma {
            fast_period,
            slow_period,
        } => {
            if i == 0 {
                return false;
            }
            let fast = match cache.sma_cache.get(fast_period) {
                Some(v) => v,
                None => return false,
            };
            let slow = match cache.sma_cache.get(slow_period) {
                Some(v) => v,
                None => return false,
            };
            match (fast[i - 1], fast[i], slow[i - 1], slow[i]) {
                (Some(fp), Some(fc), Some(sp), Some(sc)) => fp >= sp && fc < sc,
                _ => false,
            }
        }

        IndicatorCondition::EmaCrossesAboveEma {
            fast_period,
            slow_period,
        } => {
            if i == 0 {
                return false;
            }
            let fast = match cache.ema_cache.get(fast_period) {
                Some(v) => v,
                None => return false,
            };
            let slow = match cache.ema_cache.get(slow_period) {
                Some(v) => v,
                None => return false,
            };
            fast[i - 1] <= slow[i - 1] && fast[i] > slow[i]
        }

        IndicatorCondition::EmaCrossesBelowEma {
            fast_period,
            slow_period,
        } => {
            if i == 0 {
                return false;
            }
            let fast = match cache.ema_cache.get(fast_period) {
                Some(v) => v,
                None => return false,
            };
            let slow = match cache.ema_cache.get(slow_period) {
                Some(v) => v,
                None => return false,
            };
            fast[i - 1] >= slow[i - 1] && fast[i] < slow[i]
        }

        IndicatorCondition::PriceAboveSma { period } => cache
            .sma_cache
            .get(period)
            .and_then(|v| v[i])
            .map_or(false, |sma_val| cache.closes[i] > sma_val),

        IndicatorCondition::PriceBelowSma { period } => cache
            .sma_cache
            .get(period)
            .and_then(|v| v[i])
            .map_or(false, |sma_val| cache.closes[i] < sma_val),

        IndicatorCondition::PriceAboveEma { period } => cache
            .ema_cache
            .get(period)
            .map_or(false, |v| cache.closes[i] > v[i]),

        IndicatorCondition::PriceBelowEma { period } => cache
            .ema_cache
            .get(period)
            .map_or(false, |v| cache.closes[i] < v[i]),

        IndicatorCondition::MacdCrossesAboveSignal => {
            if i == 0 || cache.macd_line.is_empty() {
                return false;
            }
            cache.macd_line[i - 1] <= cache.macd_signal[i - 1]
                && cache.macd_line[i] > cache.macd_signal[i]
        }

        IndicatorCondition::MacdCrossesBelowSignal => {
            if i == 0 || cache.macd_line.is_empty() {
                return false;
            }
            cache.macd_line[i - 1] >= cache.macd_signal[i - 1]
                && cache.macd_line[i] < cache.macd_signal[i]
        }

        IndicatorCondition::MacdHistogramPositive => {
            if cache.macd_hist.is_empty() {
                return false;
            }
            cache.macd_hist[i] > 0.0
        }

        IndicatorCondition::MacdHistogramNegative => {
            if cache.macd_hist.is_empty() {
                return false;
            }
            cache.macd_hist[i] < 0.0
        }

        IndicatorCondition::PriceBelowLowerBoll { period, num_std } => {
            let key = (*period, num_std.to_bits());
            cache
                .boll_cache
                .get(&key)
                .and_then(|(_, _, lower)| lower[i])
                .map_or(false, |lb| cache.closes[i] < lb)
        }

        IndicatorCondition::PriceAboveUpperBoll { period, num_std } => {
            let key = (*period, num_std.to_bits());
            cache
                .boll_cache
                .get(&key)
                .and_then(|(_, upper, _)| upper[i])
                .map_or(false, |ub| cache.closes[i] > ub)
        }

        IndicatorCondition::PriceCrossesAboveLowerBoll { period, num_std } => {
            if i == 0 {
                return false;
            }
            let key = (*period, num_std.to_bits());
            let (_, _, lower) = match cache.boll_cache.get(&key) {
                Some(v) => v,
                None => return false,
            };
            match (lower[i - 1], lower[i]) {
                (Some(prev_lb), Some(curr_lb)) => {
                    cache.closes[i - 1] <= prev_lb && cache.closes[i] > curr_lb
                }
                _ => false,
            }
        }

        IndicatorCondition::PriceCrossesBelowUpperBoll { period, num_std } => {
            if i == 0 {
                return false;
            }
            let key = (*period, num_std.to_bits());
            let (_, upper, _) = match cache.boll_cache.get(&key) {
                Some(v) => v,
                None => return false,
            };
            match (upper[i - 1], upper[i]) {
                (Some(prev_ub), Some(curr_ub)) => {
                    cache.closes[i - 1] >= prev_ub && cache.closes[i] < curr_ub
                }
                _ => false,
            }
        }

        IndicatorCondition::PriceAbove { price } => cache.closes[i] > *price,
        IndicatorCondition::PriceBelow { price } => cache.closes[i] < *price,

        IndicatorCondition::VolumeAboveAvg { period, multiplier } => cache
            .volume_sma_cache
            .get(period)
            .and_then(|v| v[i])
            .map_or(false, |avg| cache.volumes[i] > avg * multiplier),
    }
}

fn compute_shares(
    sizing: &PositionSizing,
    capital: f64,
    price: f64,
    commission_rate: f64,
) -> u32 {
    let raw = match sizing {
        PositionSizing::FixedAmount { amount } => amount / price,
        PositionSizing::Percentage { percent } => {
            let available = capital * percent / 100.0;
            available / (price * (1.0 + commission_rate))
        }
        PositionSizing::FixedShares { shares } => *shares as f64,
    };
    // A-share: round down to nearest 100
    let lots = (raw / 100.0).floor() as u32;
    lots * 100
}

pub fn run_backtest(config: &BacktestConfig, rows: &[PriceRow]) -> Result<BacktestResult, String> {
    if rows.is_empty() {
        return Err("No price data".into());
    }

    let cache = IndicatorCache::new(rows, &config.strategy);
    let n = rows.len();

    let mut cash = config.initial_capital;
    let mut position: Option<OpenPosition> = None;
    let mut trades: Vec<Trade> = Vec::new();
    let mut equity_curve: Vec<EquityPoint> = Vec::new();

    let first_close = cache.closes[0];
    let benchmark_shares = config.initial_capital / first_close;

    let mut peak_value = config.initial_capital;

    for i in 0..n {
        let close = cache.closes[i];
        let portfolio_value = if let Some(ref pos) = position {
            cash + pos.shares as f64 * close
        } else {
            cash
        };

        if portfolio_value > peak_value {
            peak_value = portfolio_value;
        }
        let drawdown_pct = if peak_value > 0.0 {
            (peak_value - portfolio_value) / peak_value * 100.0
        } else {
            0.0
        };

        equity_curve.push(EquityPoint {
            date: rows[i].date.clone(),
            portfolio_value,
            benchmark_value: benchmark_shares * close,
            drawdown_pct,
        });

        if let Some(ref mut pos) = position {
            // T+1: can't sell on the same day we bought
            if i <= pos.entry_bar {
                continue;
            }

            let mut exit_reason: Option<ExitReason> = None;
            let mut exit_price = close;

            // Check stop loss
            if let Some(ref stop) = config.strategy.stop_loss {
                let stop_price = match stop {
                    StopRule::Percentage { percent } => {
                        pos.entry_price * (1.0 - percent / 100.0)
                    }
                    StopRule::FixedPrice { price } => *price,
                };
                if cache.lows[i] <= stop_price {
                    exit_price = stop_price;
                    exit_reason = Some(ExitReason::StopLoss);
                }
            }

            // Check take profit
            if exit_reason.is_none() {
                if let Some(ref tp) = config.strategy.take_profit {
                    let tp_price = match tp {
                        StopRule::Percentage { percent } => {
                            pos.entry_price * (1.0 + percent / 100.0)
                        }
                        StopRule::FixedPrice { price } => *price,
                    };
                    if cache.highs[i] >= tp_price {
                        exit_price = tp_price;
                        exit_reason = Some(ExitReason::TakeProfit);
                    }
                }
            }

            // Check trailing stop
            if exit_reason.is_none() {
                if let Some(ref ts) = config.strategy.trailing_stop {
                    if close > pos.highest_since_entry {
                        pos.highest_since_entry = close;
                    }
                    let trail_price = pos.highest_since_entry * (1.0 - ts.percent / 100.0);
                    if cache.lows[i] <= trail_price {
                        exit_price = trail_price;
                        exit_reason = Some(ExitReason::TrailingStop);
                    }
                }
            }

            // Check exit signal
            if exit_reason.is_none() && evaluate_group(&config.strategy.exit, i, &cache) {
                exit_reason = Some(ExitReason::Signal);
            }

            // Close position if any exit triggered
            if let Some(reason) = exit_reason {
                let sell_value = pos.shares as f64 * exit_price;
                let commission = sell_value * config.commission_rate;
                let stamp_tax = sell_value * config.stamp_tax_rate;
                cash += sell_value - commission - stamp_tax;

                let pnl = (exit_price - pos.entry_price) * pos.shares as f64
                    - pos.entry_commission
                    - commission
                    - stamp_tax;
                let pnl_pct = pnl / (pos.entry_price * pos.shares as f64) * 100.0;

                trades.push(Trade {
                    entry_date: rows[pos.entry_bar].date.clone(),
                    entry_price: pos.entry_price,
                    exit_date: rows[i].date.clone(),
                    exit_price,
                    shares: pos.shares,
                    pnl,
                    pnl_pct,
                    holding_days: (i - pos.entry_bar) as u32,
                    exit_reason: reason,
                });

                position = None;
            }
        } else {
            // No position — check entry signal
            if evaluate_group(&config.strategy.entry, i, &cache) {
                let shares =
                    compute_shares(&config.strategy.position_sizing, cash, close, config.commission_rate);
                if shares >= 100 {
                    let cost = shares as f64 * close;
                    let commission = cost * config.commission_rate;
                    cash -= cost + commission;

                    position = Some(OpenPosition {
                        entry_bar: i,
                        entry_price: close,
                        shares,
                        entry_commission: commission,
                        highest_since_entry: close,
                    });
                }
            }
        }
    }

    // Close any remaining position at end of period
    if let Some(pos) = position {
        let close = cache.closes[n - 1];
        let sell_value = pos.shares as f64 * close;
        let commission = sell_value * config.commission_rate;
        let stamp_tax = sell_value * config.stamp_tax_rate;
        cash += sell_value - commission - stamp_tax;

        let pnl = (close - pos.entry_price) * pos.shares as f64
            - pos.entry_commission
            - commission
            - stamp_tax;
        let pnl_pct = pnl / (pos.entry_price * pos.shares as f64) * 100.0;

        trades.push(Trade {
            entry_date: rows[pos.entry_bar].date.clone(),
            entry_price: pos.entry_price,
            exit_date: rows[n - 1].date.clone(),
            exit_price: close,
            shares: pos.shares,
            pnl,
            pnl_pct,
            holding_days: (n - 1 - pos.entry_bar) as u32,
            exit_reason: ExitReason::EndOfPeriod,
        });
    }

    let metrics_result = metrics::compute(&trades, &equity_curve, config.initial_capital, n);

    Ok(BacktestResult {
        config: config.clone(),
        metrics: metrics_result,
        trades,
        equity_curve,
    })
}

struct OpenPosition {
    entry_bar: usize,
    entry_price: f64,
    shares: u32,
    entry_commission: f64,
    highest_since_entry: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rows(prices: &[(f64, f64, f64, f64)]) -> Vec<PriceRow> {
        prices
            .iter()
            .enumerate()
            .map(|(i, (open, high, low, close))| PriceRow {
                date: format!("2024-01-{:02}", i + 1),
                open: *open,
                high: *high,
                low: *low,
                close: *close,
                volume: 1_000_000.0,
                amount: 10_000_000.0,
                pct_change: 0.0,
            })
            .collect()
    }

    #[test]
    fn test_basic_price_crossover() {
        // 50 bars of data with a clear price pattern
        let mut prices = Vec::new();
        for i in 0..50 {
            let base = 10.0 + (i as f64 * 0.1);
            prices.push((base, base + 0.2, base - 0.2, base));
        }
        let rows = make_rows(&prices);

        let strategy = Strategy {
            name: "test".into(),
            description: "test".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 12.0,
                })],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 14.0,
                })],
            },
            position_sizing: PositionSizing::FixedShares { shares: 100 },
            stop_loss: None,
            take_profit: None,
            trailing_stop: None,
        };

        let config = BacktestConfig {
            symbol: "000001".into(),
            start_date: "2024-01-01".into(),
            end_date: "2024-02-19".into(),
            initial_capital: 100_000.0,
            strategy,
            commission_rate: 0.0003,
            stamp_tax_rate: 0.001,
        };

        let result = run_backtest(&config, &rows).unwrap();
        assert!(!result.trades.is_empty());
        assert_eq!(result.equity_curve.len(), 50);
    }

    #[test]
    fn test_stop_loss() {
        let mut prices = Vec::new();
        // Uptrend then sharp drop
        for i in 0..20 {
            let base = 10.0 + (i as f64 * 0.5);
            prices.push((base, base + 0.3, base - 0.3, base));
        }
        for i in 0..10 {
            let base = 20.0 - (i as f64 * 1.0);
            prices.push((base, base + 0.3, base - 0.3, base));
        }
        let rows = make_rows(&prices);

        let strategy = Strategy {
            name: "test_stop".into(),
            description: "test".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 12.0,
                })],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 999.0,
                })],
            },
            position_sizing: PositionSizing::FixedShares { shares: 100 },
            stop_loss: Some(StopRule::Percentage { percent: 10.0 }),
            take_profit: None,
            trailing_stop: None,
        };

        let config = BacktestConfig {
            symbol: "000001".into(),
            start_date: "2024-01-01".into(),
            end_date: "2024-01-30".into(),
            initial_capital: 100_000.0,
            strategy,
            commission_rate: 0.0003,
            stamp_tax_rate: 0.001,
        };

        let result = run_backtest(&config, &rows).unwrap();
        let stop_trades: Vec<_> = result
            .trades
            .iter()
            .filter(|t| matches!(t.exit_reason, ExitReason::StopLoss))
            .collect();
        assert!(
            !stop_trades.is_empty(),
            "Expected at least one stop loss trade"
        );
    }

    #[test]
    fn test_empty_data() {
        let rows: Vec<PriceRow> = vec![];
        let config = BacktestConfig {
            symbol: "000001".into(),
            start_date: "2024-01-01".into(),
            end_date: "2024-01-30".into(),
            initial_capital: 100_000.0,
            strategy: Strategy {
                name: "test".into(),
                description: "test".into(),
                entry: ConditionGroup {
                    logic: Logic::And,
                    conditions: vec![],
                },
                exit: ConditionGroup {
                    logic: Logic::And,
                    conditions: vec![],
                },
                position_sizing: PositionSizing::Percentage { percent: 100.0 },
                stop_loss: None,
                take_profit: None,
                trailing_stop: None,
            },
            commission_rate: 0.0003,
            stamp_tax_rate: 0.001,
        };
        assert!(run_backtest(&config, &rows).is_err());
    }

    #[test]
    fn test_a_share_lot_rounding() {
        let shares = compute_shares(
            &PositionSizing::Percentage { percent: 100.0 },
            10_000.0,
            10.0,
            0.0003,
        );
        // 10000 / (10 * 1.0003) ≈ 999.7, rounded down to 900
        assert_eq!(shares, 900);
    }

    #[test]
    fn test_t_plus_1() {
        // Create data where entry and immediate exit conditions are both met
        let mut prices = Vec::new();
        for _ in 0..10 {
            prices.push((5.0, 5.5, 4.5, 5.0));
        }
        let rows = make_rows(&prices);

        let strategy = Strategy {
            name: "test_t1".into(),
            description: "test".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 4.0,
                })],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 4.0,
                })],
            },
            position_sizing: PositionSizing::FixedShares { shares: 100 },
            stop_loss: None,
            take_profit: None,
            trailing_stop: None,
        };

        let config = BacktestConfig {
            symbol: "000001".into(),
            start_date: "2024-01-01".into(),
            end_date: "2024-01-10".into(),
            initial_capital: 100_000.0,
            strategy,
            commission_rate: 0.0003,
            stamp_tax_rate: 0.001,
        };

        let result = run_backtest(&config, &rows).unwrap();
        // With T+1, each trade should hold at least 1 day
        for trade in &result.trades {
            assert!(trade.holding_days >= 1, "T+1 violation: holding_days < 1");
            assert_ne!(trade.entry_date, trade.exit_date, "T+1 violation: same-day exit");
        }
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut prices = Vec::new();
        for i in 0..50 {
            let base = 10.0 + (i as f64 * 0.1);
            prices.push((base, base + 0.2, base - 0.2, base));
        }
        let rows = make_rows(&prices);

        let strategy = Strategy {
            name: "test".into(),
            description: "test".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 12.0,
                })],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::PriceAbove {
                    price: 14.0,
                })],
            },
            position_sizing: PositionSizing::FixedShares { shares: 100 },
            stop_loss: Some(StopRule::Percentage { percent: 10.0 }),
            take_profit: None,
            trailing_stop: None,
        };

        // Test strategy JSON roundtrip (frontend → backend)
        let strategy_json = serde_json::to_string(&strategy).unwrap();
        let _: Strategy = serde_json::from_str(&strategy_json).unwrap();

        let config = BacktestConfig {
            symbol: "000001".into(),
            start_date: "2024-01-01".into(),
            end_date: "2024-02-19".into(),
            initial_capital: 100_000.0,
            strategy,
            commission_rate: 0.0003,
            stamp_tax_rate: 0.001,
        };

        let result = run_backtest(&config, &rows).unwrap();

        // Test result serialization (backend → frontend)
        let value = serde_json::to_value(&result).unwrap();
        let json_str = serde_json::to_string(&value).unwrap();

        // Verify camelCase keys in the serialized JSON
        assert!(json_str.contains("equityCurve"), "Expected camelCase key equityCurve");
        assert!(json_str.contains("totalReturnPct"), "Expected camelCase key totalReturnPct");
        assert!(json_str.contains("entryDate"), "Expected camelCase key entryDate");
        assert!(json_str.contains("exitReason"), "Expected camelCase key exitReason");

        // Strategy inside config should still have snake_case (Strategy has no rename_all)
        assert!(json_str.contains("position_sizing"), "Strategy should keep snake_case");

        // Verify profit_factor is finite and serializable (not null from Infinity)
        let pf = value["metrics"]["profitFactor"].as_f64();
        assert!(pf.is_some(), "profitFactor must be finite, got: {:?}", pf);
        assert!(pf.unwrap().is_finite(), "profitFactor must not be Infinity/NaN");
    }
}
