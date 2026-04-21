use log::info;
use serde_json::{json, Value};

use crate::backtest::strategy::Strategy;
use crate::providers::{LLMProvider, Message};

const MAX_RETRIES: usize = 3;

const SYSTEM_PROMPT: &str = r#"你是一个交易策略翻译器。用户会用自然语言（中文或英文）描述一个交易策略，你需要将其转换为结构化的策略定义，然后调用 submit_strategy 工具提交。

## 策略结构

一个策略包含：
- name: 策略名称
- description: 策略描述
- entry: 入场条件组（ConditionGroup）
- exit: 出场条件组（ConditionGroup）
- position_sizing: 仓位管理
- stop_loss: 止损规则（可选）
- take_profit: 止盈规则（可选）
- trailing_stop: 移动止损（可选）

## 条件组 (ConditionGroup)

```json
{
  "logic": "and" 或 "or",
  "conditions": [条件数组]
}
```

每个条件可以是指标条件或嵌套条件组：
- 指标条件：`{"type": "indicator", "indicator": "指标名", "params": {参数}}`
- 嵌套组：`{"type": "group", "logic": "and/or", "conditions": [...]}`

## 支持的指标条件

### RSI（相对强弱指标）
- `rsi_above`: RSI高于阈值。参数: `{"period": 14, "threshold": 70}`
- `rsi_below`: RSI低于阈值。参数: `{"period": 14, "threshold": 30}`
- `rsi_crosses_above`: RSI从下方穿越阈值。参数: `{"period": 14, "threshold": 70}`
- `rsi_crosses_below`: RSI从上方穿越阈值。参数: `{"period": 14, "threshold": 30}`

### 均线交叉
- `sma_crosses_above_sma`: 快速SMA上穿慢速SMA（金叉）。参数: `{"fast_period": 20, "slow_period": 50}`
- `sma_crosses_below_sma`: 快速SMA下穿慢速SMA（死叉）。参数: `{"fast_period": 20, "slow_period": 50}`
- `ema_crosses_above_ema`: 快速EMA上穿慢速EMA。参数: `{"fast_period": 12, "slow_period": 26}`
- `ema_crosses_below_ema`: 快速EMA下穿慢速EMA。参数: `{"fast_period": 12, "slow_period": 26}`

### 价格与均线
- `price_above_sma`: 价格在SMA上方。参数: `{"period": 20}`
- `price_below_sma`: 价格在SMA下方。参数: `{"period": 20}`
- `price_above_ema`: 价格在EMA上方。参数: `{"period": 20}`
- `price_below_ema`: 价格在EMA下方。参数: `{"period": 20}`

### MACD
- `macd_crosses_above_signal`: MACD线上穿信号线（金叉）。无参数。
- `macd_crosses_below_signal`: MACD线下穿信号线（死叉）。无参数。
- `macd_histogram_positive`: MACD柱状图为正。无参数。
- `macd_histogram_negative`: MACD柱状图为负。无参数。

注意：无参数的 MACD 指标不需要 "params" 字段，直接写 `{"type": "indicator", "indicator": "macd_crosses_above_signal"}`

### 布林带
- `price_below_lower_boll`: 价格低于布林带下轨。参数: `{"period": 20, "num_std": 2.0}`
- `price_above_upper_boll`: 价格高于布林带上轨。参数: `{"period": 20, "num_std": 2.0}`
- `price_crosses_above_lower_boll`: 价格从下方穿越布林带下轨（反弹信号）。参数: `{"period": 20, "num_std": 2.0}`
- `price_crosses_below_upper_boll`: 价格从上方穿越布林带上轨。参数: `{"period": 20, "num_std": 2.0}`

### 价格水平
- `price_above`: 价格高于指定值。参数: `{"price": 100.0}`
- `price_below`: 价格低于指定值。参数: `{"price": 50.0}`

### 成交量
- `volume_above_avg`: 成交量高于均量的倍数。参数: `{"period": 20, "multiplier": 1.5}`

## 仓位管理 (position_sizing)

- 按百分比: `{"type": "percentage", "percent": 100.0}`
- 固定金额: `{"type": "fixed_amount", "amount": 50000.0}`
- 固定股数: `{"type": "fixed_shares", "shares": 1000}`

## 止损/止盈规则 (stop_loss / take_profit)

- 百分比: `{"type": "percentage", "percent": 10.0}`
- 固定价格: `{"type": "fixed_price", "price": 45.0}`

## 移动止损 (trailing_stop)

`{"percent": 8.0}` — 从最高价回撤8%触发止损

## 示例

用户说："当RSI低于30且价格在60日均线上方时买入，RSI高于70时卖出，设置5%止损"

对应的策略：
```json
{
  "name": "RSI超卖+均线过滤",
  "description": "RSI(14)<30且价格在SMA(60)上方时买入，RSI(14)>70时卖出，5%止损",
  "entry": {
    "logic": "and",
    "conditions": [
      {"type": "indicator", "indicator": "rsi_below", "params": {"period": 14, "threshold": 30}},
      {"type": "indicator", "indicator": "price_above_sma", "params": {"period": 60}}
    ]
  },
  "exit": {
    "logic": "and",
    "conditions": [
      {"type": "indicator", "indicator": "rsi_above", "params": {"period": 14, "threshold": 70}}
    ]
  },
  "position_sizing": {"type": "percentage", "percent": 100},
  "stop_loss": {"type": "percentage", "percent": 5},
  "take_profit": null,
  "trailing_stop": null
}
```

## 重要规则

1. 如果用户描述的条件无法用上述支持的指标表示，请在回复中说明哪些条件不被支持，不要调用 submit_strategy
2. 为策略起一个简短的中文名称
3. period 参数必须为正整数，threshold/percent/price 参数必须为正数
4. 如果用户没有指定仓位管理，默认使用 100% 仓位
5. 如果用户没有明确指定止损/止盈/移动止损，对应字段设为 null
6. 请仔细选择"crosses"类型（穿越，适合入场信号）和"above/below"类型（状态，适合持续条件）的区别
"#;

fn strategy_tool() -> Vec<Value> {
    vec![json!({
        "type": "function",
        "function": {
            "name": "submit_strategy",
            "description": "Submit a validated trading strategy definition translated from natural language",
            "parameters": {
                "type": "object",
                "properties": {
                    "strategy": {
                        "type": "object",
                        "description": "The complete strategy definition",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" },
                            "entry": { "type": "object", "description": "Entry ConditionGroup" },
                            "exit": { "type": "object", "description": "Exit ConditionGroup" },
                            "position_sizing": { "type": "object" },
                            "stop_loss": { "type": ["object", "null"] },
                            "take_profit": { "type": ["object", "null"] },
                            "trailing_stop": { "type": ["object", "null"] }
                        },
                        "required": ["name", "description", "entry", "exit", "position_sizing"]
                    }
                },
                "required": ["strategy"]
            }
        }
    })]
}

pub async fn translate_strategy(
    provider: &dyn LLMProvider,
    user_description: &str,
) -> Result<(Strategy, String), String> {
    let tools = strategy_tool();
    let mut messages: Vec<Message> = vec![Message {
        role: "user".into(),
        content: Some(user_description.into()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];

    for attempt in 0..MAX_RETRIES {
        info!(
            "[strategy_translator] Attempt {}/{}",
            attempt + 1,
            MAX_RETRIES
        );

        let response = provider
            .complete(SYSTEM_PROMPT, &messages, &tools, 4096)
            .await?;

        if !response.has_tool_calls() {
            return Err(format!(
                "LLM did not produce a strategy. Response: {}",
                response.content
            ));
        }

        let tc = &response.tool_calls[0];
        if tc.name != "submit_strategy" {
            return Err(format!("Unexpected tool call: {}", tc.name));
        }

        let strategy_value = tc
            .arguments
            .get("strategy")
            .ok_or("Tool call missing 'strategy' field")?;

        match serde_json::from_value::<Strategy>(strategy_value.clone()) {
            Ok(strategy) => {
                let explanation = if response.content.is_empty() {
                    format!("已将策略翻译为: {}", strategy.name)
                } else {
                    response.content.clone()
                };
                info!(
                    "[strategy_translator] Success: {}",
                    strategy.name
                );
                return Ok((strategy, explanation));
            }
            Err(e) => {
                let error_msg = format!(
                    "Strategy JSON validation failed: {e}. \
                     Please fix the JSON and call submit_strategy again."
                );
                info!("[strategy_translator] Validation error: {e}");

                let tool_calls_json = response
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                            }
                        })
                    })
                    .collect();

                messages.push(Message {
                    role: "assistant".into(),
                    content: if response.content.is_empty() {
                        None
                    } else {
                        Some(response.content.clone())
                    },
                    tool_calls: Some(tool_calls_json),
                    tool_call_id: None,
                    name: None,
                });

                messages.push(Message {
                    role: "tool".into(),
                    content: Some(error_msg),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    name: Some("submit_strategy".into()),
                });
            }
        }
    }

    Err("Failed to translate strategy after maximum retries".into())
}
