use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    pub name: String,
    pub description: String,
    pub entry: ConditionGroup,
    pub exit: ConditionGroup,
    pub position_sizing: PositionSizing,
    pub stop_loss: Option<StopRule>,
    pub take_profit: Option<StopRule>,
    pub trailing_stop: Option<TrailingStop>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionGroup {
    pub logic: Logic,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Logic {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Condition {
    #[serde(rename = "indicator")]
    Indicator(IndicatorCondition),
    #[serde(rename = "group")]
    Group(ConditionGroup),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "indicator", content = "params")]
pub enum IndicatorCondition {
    // RSI
    #[serde(rename = "rsi_above")]
    RsiAbove { period: usize, threshold: f64 },
    #[serde(rename = "rsi_below")]
    RsiBelow { period: usize, threshold: f64 },
    #[serde(rename = "rsi_crosses_above")]
    RsiCrossesAbove { period: usize, threshold: f64 },
    #[serde(rename = "rsi_crosses_below")]
    RsiCrossesBelow { period: usize, threshold: f64 },

    // Moving average crossovers
    #[serde(rename = "sma_crosses_above_sma")]
    SmaCrossesAboveSma {
        fast_period: usize,
        slow_period: usize,
    },
    #[serde(rename = "sma_crosses_below_sma")]
    SmaCrossesBelowSma {
        fast_period: usize,
        slow_period: usize,
    },
    #[serde(rename = "ema_crosses_above_ema")]
    EmaCrossesAboveEma {
        fast_period: usize,
        slow_period: usize,
    },
    #[serde(rename = "ema_crosses_below_ema")]
    EmaCrossesBelowEma {
        fast_period: usize,
        slow_period: usize,
    },

    // Price vs moving average
    #[serde(rename = "price_above_sma")]
    PriceAboveSma { period: usize },
    #[serde(rename = "price_below_sma")]
    PriceBelowSma { period: usize },
    #[serde(rename = "price_above_ema")]
    PriceAboveEma { period: usize },
    #[serde(rename = "price_below_ema")]
    PriceBelowEma { period: usize },

    // MACD
    #[serde(rename = "macd_crosses_above_signal")]
    MacdCrossesAboveSignal,
    #[serde(rename = "macd_crosses_below_signal")]
    MacdCrossesBelowSignal,
    #[serde(rename = "macd_histogram_positive")]
    MacdHistogramPositive,
    #[serde(rename = "macd_histogram_negative")]
    MacdHistogramNegative,

    // Bollinger Bands
    #[serde(rename = "price_below_lower_boll")]
    PriceBelowLowerBoll { period: usize, num_std: f64 },
    #[serde(rename = "price_above_upper_boll")]
    PriceAboveUpperBoll { period: usize, num_std: f64 },
    #[serde(rename = "price_crosses_above_lower_boll")]
    PriceCrossesAboveLowerBoll { period: usize, num_std: f64 },
    #[serde(rename = "price_crosses_below_upper_boll")]
    PriceCrossesBelowUpperBoll { period: usize, num_std: f64 },

    // Price level
    #[serde(rename = "price_above")]
    PriceAbove { price: f64 },
    #[serde(rename = "price_below")]
    PriceBelow { price: f64 },

    // Volume
    #[serde(rename = "volume_above_avg")]
    VolumeAboveAvg { period: usize, multiplier: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PositionSizing {
    #[serde(rename = "fixed_amount")]
    FixedAmount { amount: f64 },
    #[serde(rename = "percentage")]
    Percentage { percent: f64 },
    #[serde(rename = "fixed_shares")]
    FixedShares { shares: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StopRule {
    #[serde(rename = "percentage")]
    Percentage { percent: f64 },
    #[serde(rename = "fixed_price")]
    FixedPrice { price: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrailingStop {
    pub percent: f64,
}
