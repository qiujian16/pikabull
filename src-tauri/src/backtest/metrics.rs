use super::engine::{EquityPoint, Trade};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestMetrics {
    pub total_return_pct: f64,
    pub annualized_return_pct: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown_pct: f64,
    pub max_drawdown_duration_days: u32,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub avg_return_per_trade_pct: f64,
    pub total_trades: u32,
    pub winning_trades: u32,
    pub losing_trades: u32,
    pub avg_holding_days: f64,
    pub benchmark_return_pct: f64,
}

pub fn compute(
    trades: &[Trade],
    equity_curve: &[EquityPoint],
    initial_capital: f64,
    trading_days: usize,
) -> BacktestMetrics {
    let total_trades = trades.len() as u32;
    let winning_trades = trades.iter().filter(|t| t.pnl > 0.0).count() as u32;
    let losing_trades = trades.iter().filter(|t| t.pnl < 0.0).count() as u32;

    let win_rate = if total_trades > 0 {
        winning_trades as f64 / total_trades as f64
    } else {
        0.0
    };

    let avg_return_per_trade_pct = if total_trades > 0 {
        trades.iter().map(|t| t.pnl_pct).sum::<f64>() / total_trades as f64
    } else {
        0.0
    };

    let avg_holding_days = if total_trades > 0 {
        trades.iter().map(|t| t.holding_days as f64).sum::<f64>() / total_trades as f64
    } else {
        0.0
    };

    let gross_profit: f64 = trades.iter().filter(|t| t.pnl > 0.0).map(|t| t.pnl).sum();
    let gross_loss: f64 = trades
        .iter()
        .filter(|t| t.pnl < 0.0)
        .map(|t| t.pnl.abs())
        .sum();
    let profit_factor = if gross_loss > 0.0 {
        gross_profit / gross_loss
    } else if gross_profit > 0.0 {
        f64::MAX
    } else {
        0.0
    };

    let final_value = equity_curve
        .last()
        .map(|e| e.portfolio_value)
        .unwrap_or(initial_capital);
    let total_return_pct = (final_value / initial_capital - 1.0) * 100.0;

    let years = trading_days as f64 / 252.0;
    let annualized_return_pct = if years > 0.0 {
        ((final_value / initial_capital).powf(1.0 / years) - 1.0) * 100.0
    } else {
        0.0
    };

    let benchmark_return_pct = equity_curve
        .last()
        .map(|e| (e.benchmark_value / initial_capital - 1.0) * 100.0)
        .unwrap_or(0.0);

    let (max_drawdown_pct, max_drawdown_duration_days) = compute_max_drawdown(equity_curve);
    let sharpe_ratio = compute_sharpe(equity_curve);

    BacktestMetrics {
        total_return_pct,
        annualized_return_pct,
        sharpe_ratio,
        max_drawdown_pct,
        max_drawdown_duration_days,
        win_rate,
        profit_factor,
        avg_return_per_trade_pct,
        total_trades,
        winning_trades,
        losing_trades,
        avg_holding_days,
        benchmark_return_pct,
    }
}

fn compute_max_drawdown(equity_curve: &[EquityPoint]) -> (f64, u32) {
    if equity_curve.is_empty() {
        return (0.0, 0);
    }

    let mut peak = equity_curve[0].portfolio_value;
    let mut max_dd = 0.0_f64;
    let mut peak_idx = 0usize;
    let mut max_dd_duration = 0u32;
    let mut current_dd_start = 0usize;

    for (i, point) in equity_curve.iter().enumerate() {
        if point.portfolio_value > peak {
            peak = point.portfolio_value;
            peak_idx = i;
            current_dd_start = i;
        }
        let dd = (peak - point.portfolio_value) / peak * 100.0;
        if dd > max_dd {
            max_dd = dd;
            max_dd_duration = (i - current_dd_start) as u32;
        }
        let _ = peak_idx; // used for duration tracking
    }

    (max_dd, max_dd_duration)
}

fn compute_sharpe(equity_curve: &[EquityPoint]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }

    let daily_returns: Vec<f64> = equity_curve
        .windows(2)
        .map(|w| w[1].portfolio_value / w[0].portfolio_value - 1.0)
        .collect();

    if daily_returns.is_empty() {
        return 0.0;
    }

    let mean = daily_returns.iter().sum::<f64>() / daily_returns.len() as f64;
    let variance =
        daily_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_returns.len() as f64;
    let std_dev = variance.sqrt();

    if std_dev == 0.0 {
        return 0.0;
    }

    (mean / std_dev) * 252.0_f64.sqrt()
}
