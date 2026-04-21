use super::strategy::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetStrategy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub params: Vec<PresetParam>,
    pub strategy: Strategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetParam {
    pub key: String,
    pub label: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

pub fn list_presets() -> Vec<PresetStrategy> {
    vec![
        golden_cross(),
        rsi_mean_reversion(),
        macd_momentum(),
        bollinger_bounce(),
        dual_ma_rsi_filter(),
    ]
}

fn golden_cross() -> PresetStrategy {
    PresetStrategy {
        id: "golden_cross".into(),
        name: "金叉策略 (Golden Cross)".into(),
        description: "快速均线上穿慢速均线买入，下穿卖出".into(),
        params: vec![
            PresetParam {
                key: "fast_period".into(),
                label: "快速均线周期".into(),
                default: 20.0,
                min: 5.0,
                max: 60.0,
                step: 1.0,
            },
            PresetParam {
                key: "slow_period".into(),
                label: "慢速均线周期".into(),
                default: 50.0,
                min: 20.0,
                max: 200.0,
                step: 1.0,
            },
        ],
        strategy: Strategy {
            name: "金叉策略".into(),
            description: "SMA(20)上穿SMA(50)买入，下穿卖出".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(
                    IndicatorCondition::SmaCrossesAboveSma {
                        fast_period: 20,
                        slow_period: 50,
                    },
                )],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(
                    IndicatorCondition::SmaCrossesBelowSma {
                        fast_period: 20,
                        slow_period: 50,
                    },
                )],
            },
            position_sizing: PositionSizing::Percentage { percent: 100.0 },
            stop_loss: Some(StopRule::Percentage { percent: 10.0 }),
            take_profit: None,
            trailing_stop: None,
        },
    }
}

fn rsi_mean_reversion() -> PresetStrategy {
    PresetStrategy {
        id: "rsi_mean_reversion".into(),
        name: "RSI均值回归".into(),
        description: "RSI超卖时买入，超买时卖出".into(),
        params: vec![
            PresetParam {
                key: "period".into(),
                label: "RSI周期".into(),
                default: 14.0,
                min: 5.0,
                max: 30.0,
                step: 1.0,
            },
            PresetParam {
                key: "oversold".into(),
                label: "超卖阈值".into(),
                default: 30.0,
                min: 10.0,
                max: 40.0,
                step: 1.0,
            },
            PresetParam {
                key: "overbought".into(),
                label: "超买阈值".into(),
                default: 70.0,
                min: 60.0,
                max: 90.0,
                step: 1.0,
            },
        ],
        strategy: Strategy {
            name: "RSI均值回归".into(),
            description: "RSI(14)<30买入，RSI(14)>70卖出".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::RsiCrossesBelow {
                    period: 14,
                    threshold: 30.0,
                })],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(IndicatorCondition::RsiCrossesAbove {
                    period: 14,
                    threshold: 70.0,
                })],
            },
            position_sizing: PositionSizing::Percentage { percent: 100.0 },
            stop_loss: Some(StopRule::Percentage { percent: 8.0 }),
            take_profit: None,
            trailing_stop: None,
        },
    }
}

fn macd_momentum() -> PresetStrategy {
    PresetStrategy {
        id: "macd_momentum".into(),
        name: "MACD动量".into(),
        description: "MACD金叉买入，死叉卖出".into(),
        params: vec![],
        strategy: Strategy {
            name: "MACD动量".into(),
            description: "MACD线上穿信号线买入，下穿卖出".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(
                    IndicatorCondition::MacdCrossesAboveSignal,
                )],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(
                    IndicatorCondition::MacdCrossesBelowSignal,
                )],
            },
            position_sizing: PositionSizing::Percentage { percent: 100.0 },
            stop_loss: Some(StopRule::Percentage { percent: 8.0 }),
            take_profit: None,
            trailing_stop: None,
        },
    }
}

fn bollinger_bounce() -> PresetStrategy {
    PresetStrategy {
        id: "bollinger_bounce".into(),
        name: "布林带反弹".into(),
        description: "价格触及下轨买入，触及上轨卖出".into(),
        params: vec![
            PresetParam {
                key: "period".into(),
                label: "布林带周期".into(),
                default: 20.0,
                min: 10.0,
                max: 50.0,
                step: 1.0,
            },
            PresetParam {
                key: "num_std".into(),
                label: "标准差倍数".into(),
                default: 2.0,
                min: 1.0,
                max: 3.0,
                step: 0.1,
            },
        ],
        strategy: Strategy {
            name: "布林带反弹".into(),
            description: "价格从下轨反弹买入，触及上轨卖出".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(
                    IndicatorCondition::PriceCrossesAboveLowerBoll {
                        period: 20,
                        num_std: 2.0,
                    },
                )],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(
                    IndicatorCondition::PriceAboveUpperBoll {
                        period: 20,
                        num_std: 2.0,
                    },
                )],
            },
            position_sizing: PositionSizing::Percentage { percent: 100.0 },
            stop_loss: Some(StopRule::Percentage { percent: 5.0 }),
            take_profit: None,
            trailing_stop: None,
        },
    }
}

fn dual_ma_rsi_filter() -> PresetStrategy {
    PresetStrategy {
        id: "dual_ma_rsi".into(),
        name: "双均线+RSI过滤".into(),
        description: "均线金叉且RSI未超买时买入，带移动止损".into(),
        params: vec![
            PresetParam {
                key: "fast_period".into(),
                label: "快速均线周期".into(),
                default: 10.0,
                min: 5.0,
                max: 30.0,
                step: 1.0,
            },
            PresetParam {
                key: "slow_period".into(),
                label: "慢速均线周期".into(),
                default: 30.0,
                min: 20.0,
                max: 100.0,
                step: 1.0,
            },
            PresetParam {
                key: "rsi_threshold".into(),
                label: "RSI过滤阈值".into(),
                default: 50.0,
                min: 30.0,
                max: 70.0,
                step: 1.0,
            },
            PresetParam {
                key: "trailing_stop_pct".into(),
                label: "移动止损%".into(),
                default: 8.0,
                min: 3.0,
                max: 15.0,
                step: 0.5,
            },
        ],
        strategy: Strategy {
            name: "双均线+RSI过滤".into(),
            description: "EMA(10)上穿EMA(30)且RSI<50时买入，下穿卖出，8%移动止损".into(),
            entry: ConditionGroup {
                logic: Logic::And,
                conditions: vec![
                    Condition::Indicator(IndicatorCondition::EmaCrossesAboveEma {
                        fast_period: 10,
                        slow_period: 30,
                    }),
                    Condition::Indicator(IndicatorCondition::RsiBelow {
                        period: 14,
                        threshold: 50.0,
                    }),
                ],
            },
            exit: ConditionGroup {
                logic: Logic::And,
                conditions: vec![Condition::Indicator(
                    IndicatorCondition::EmaCrossesBelowEma {
                        fast_period: 10,
                        slow_period: 30,
                    },
                )],
            },
            position_sizing: PositionSizing::Percentage { percent: 100.0 },
            stop_loss: None,
            take_profit: None,
            trailing_stop: Some(TrailingStop { percent: 8.0 }),
        },
    }
}
