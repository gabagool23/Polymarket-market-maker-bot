use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MarketRow {
    pub condition_id: String,
    pub question: String,
    pub token1: String,
    pub token2: String,
    pub answer1: String,
    pub answer2: String,
    pub tick_size: f64,
    pub min_size: f64,
    pub trade_size: f64,
    pub max_size: Option<f64>,
    pub max_spread: f64,
    pub neg_risk: String,
    pub best_bid: f64,
    pub best_ask: f64,
    pub param_type: String,
    pub multiplier: String,
    /// 3-hour volatility metric
    pub three_hour: f64,
}

impl MarketRow {
    pub fn max_size(&self) -> f64 {
        self.max_size.unwrap_or(self.trade_size)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OrderSide {
    pub price: f64,
    pub size: f64,
}

#[derive(Debug, Clone, Default)]
pub struct OrderState {
    pub buy: OrderSide,
    pub sell: OrderSide,
}

#[derive(Debug, Clone, Default)]
pub struct Position {
    pub size: f64,
    pub avg_price: f64,
}

#[derive(Debug, Clone)]
pub struct BestBidAsk {
    pub best_bid: Option<f64>,
    pub best_bid_size: Option<f64>,
    pub second_best_bid: Option<f64>,
    pub second_best_bid_size: Option<f64>,
    pub top_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub best_ask_size: Option<f64>,
    pub second_best_ask: Option<f64>,
    pub second_best_ask_size: Option<f64>,
    pub top_ask: Option<f64>,
    pub bid_sum_within_n_percent: f64,
    pub ask_sum_within_n_percent: f64,
}

/// Hyperparameters from Google Sheets (by param_type)
pub type ParamsMap = HashMap<String, HashMap<String, serde_json::Value>>;

#[derive(Debug, Clone, Deserialize)]
pub struct SheetMarket {
    pub question: Option<String>,
    pub condition_id: Option<String>,
    pub token1: Option<String>,
    pub token2: Option<String>,
    pub answer1: Option<String>,
    pub answer2: Option<String>,
    pub tick_size: Option<f64>,
    pub min_size: Option<f64>,
    pub trade_size: Option<f64>,
    pub max_size: Option<f64>,
    pub max_spread: Option<f64>,
    pub neg_risk: Option<String>,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub param_type: Option<String>,
    pub multiplier: Option<String>,
    #[serde(rename = "3_hour")]
    pub three_hour: Option<f64>,
}
