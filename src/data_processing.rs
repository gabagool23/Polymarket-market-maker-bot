//! Process WebSocket messages: order book updates and user events.

use std::time::SystemTime;

use ordered_float::OrderedFloat;
use serde::Deserialize;

use crate::data_utils::{set_order, set_position};
use crate::state::{GlobalState, MarketData};
use crate::types::OrderState;

#[derive(Debug, Deserialize)]
pub struct BookEntry {
    pub price: String,
    pub size: String,
}

#[derive(Debug, Deserialize)]
pub struct BookMessage {
    pub event_type: String,
    pub market: String,
    pub asset_id: Option<String>,
    pub bids: Vec<BookEntry>,
    pub asks: Vec<BookEntry>,
}

#[derive(Debug, Deserialize)]
pub struct PriceChange {
    pub side: String,
    pub price: String,
    pub size: String,
    pub asset_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PriceChangeMessage {
    pub event_type: String,
    pub market: String,
    pub price_changes: Vec<PriceChange>,
}

#[derive(Debug, Deserialize)]
pub struct UserEventRow {
    pub event_type: String,
    pub market: String,
    pub side: String,
    pub asset_id: String,
    pub id: Option<String>,
    pub status: Option<String>,
    pub outcome: Option<String>,
    pub size: Option<String>,
    pub price: Option<String>,
    pub maker_orders: Option<Vec<MakerOrder>>,
}

#[derive(Debug, Deserialize)]
pub struct MakerOrder {
    pub maker_address: String,
    pub matched_amount: Option<String>,
    pub price: Option<String>,
    pub outcome: Option<String>,
}

pub fn process_book_data(state: &mut GlobalState, asset: &str, json_data: &BookMessage) {
    let mut bids = std::collections::BTreeMap::new();
    let mut asks = std::collections::BTreeMap::new();
    for e in &json_data.bids {
        if let (Ok(p), Ok(s)) = (e.price.parse::<f64>(), e.size.parse::<f64>()) {
            bids.insert(OrderedFloat(p), s);
        }
    }
    for e in &json_data.asks {
        if let (Ok(p), Ok(s)) = (e.price.parse::<f64>(), e.size.parse::<f64>()) {
            asks.insert(OrderedFloat(p), s);
        }
    }
    let asset_id = json_data.asset_id.clone().unwrap_or_else(|| asset.to_string());
    state.all_data.insert(
        asset.to_string(),
        MarketData {
            asset_id,
            bids,
            asks,
        },
    );
}

pub fn process_price_change(
    state: &mut GlobalState,
    asset: &str,
    side: &str,
    price_level: f64,
    new_size: f64,
    asset_id: Option<&str>,
) {
    let Some(data) = state.all_data.get_mut(asset) else { return };
    if let Some(ref aid) = asset_id {
        if data.asset_id != *aid {
            return;
        }
    }
    let book = if side == "bids" {
        &mut data.bids
    } else {
        &mut data.asks
    };
    let key = OrderedFloat(price_level);
    if new_size == 0.0 {
        book.remove(&key);
    } else {
        book.insert(key, new_size);
    }
}

/// Returns market id(s) that should trigger perform_trade.
pub fn process_data(
    state: &mut GlobalState,
    json_data: &[serde_json::Value],
    trade: bool,
) -> Vec<String> {
    let mut to_trade = Vec::new();
    for msg in json_data {
        let event_type = msg["event_type"].as_str().unwrap_or("");
        let market = msg["market"].as_str().unwrap_or("").to_string();

        if event_type == "book" {
            if let Ok(book) = serde_json::from_value::<BookMessage>(msg.clone()) {
                process_book_data(state, &market, &book);
                if trade {
                    to_trade.push(market);
                }
            }
        } else if event_type == "price_change" {
            if let Some(pc) = msg["price_changes"].as_array() {
                for data in pc {
                    let side = if data["side"].as_str().unwrap_or("") == "BUY" {
                        "bids"
                    } else {
                        "asks"
                    };
                    let price_level: f64 = data["price"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let new_size: f64 = data["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let asset_id = data["asset_id"].as_str();
                    process_price_change(state, &market, side, price_level, new_size, asset_id);
                }
                if trade {
                    to_trade.push(market);
                }
            }
        }
    }
    to_trade
}

pub fn add_to_performing(state: &mut GlobalState, col: &str, id: &str) {
    state.performing.entry(col.to_string()).or_default().insert(id.to_string());
    state
        .performing_timestamps
        .entry(col.to_string())
        .or_default()
        .insert(id.to_string(), SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64());
}

pub fn remove_from_performing(state: &mut GlobalState, col: &str, id: &str) {
    state.performing.entry(col.to_string()).or_default().remove(id);
    state.performing_timestamps.entry(col.to_string()).or_default().remove(id);
}

/// Process user WebSocket events. Returns list of market ids to run perform_trade on.
pub fn process_user_data(state: &mut GlobalState, rows: &[serde_json::Value], wallet_lower: &str) -> Vec<String> {
    let mut to_trade = Vec::new();
    for row in rows {
        let market = row["market"].as_str().unwrap_or("").to_string();
        let side = row["side"].as_str().unwrap_or("").to_lowercase();
        let token = row["asset_id"].as_str().unwrap_or("").to_string();

        if !state.reverse_tokens.contains_key(&token) {
            continue;
        }

        let col = format!("{}_{}", token, side);

        if row["event_type"].as_str().unwrap_or("") == "trade" {
            let size: f64 = row["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let price: f64 = row["price"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let mut processed_side = side.clone();
            let mut processed_token = token.clone();
            let maker_orders = row["maker_orders"].as_array();
            let mut is_user_maker = false;
            if let Some(mo) = maker_orders {
                for m in mo {
                    if m["maker_address"].as_str().map(|s| s.to_lowercase()) == Some(wallet_lower.to_string()) {
                        let matched: f64 = m["matched_amount"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                        let p: f64 = m["price"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                        let maker_outcome = m["outcome"].as_str().unwrap_or("");
                        let taker_outcome = row["outcome"].as_str().unwrap_or("");
                        if maker_outcome == taker_outcome {
                            processed_side = if side == "sell" { "buy".to_string() } else { "sell".to_string() };
                        } else {
                            processed_token = state.reverse_tokens.get(&token).cloned().unwrap_or(token);
                        }
                        is_user_maker = true;
                        break;
                    }
                }
            }
            let (size_f, price_f) = if is_user_maker {
                (size, price)
            } else {
                (row["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                 row["price"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0))
            };

            let status = row["status"].as_str().unwrap_or("");
            if status == "CONFIRMED" || status == "FAILED" {
                if status == "CONFIRMED" {
                    if let Some(id) = row["id"].as_str() {
                        remove_from_performing(state, &col, id);
                    }
                    to_trade.push(market);
                }
            } else if status == "MATCHED" {
                if let Some(id) = row["id"].as_str() {
                    add_to_performing(state, &col, id);
                }
                set_position(state, &processed_token, &processed_side, size_f, price_f, "websocket");
                to_trade.push(market);
            } else if status == "MINED" {
                if let Some(id) = row["id"].as_str() {
                    remove_from_performing(state, &col, id);
                }
            }
        } else if row["event_type"].as_str().unwrap_or("") == "order" {
            let orig: f64 = row["original_size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let matched: f64 = row["size_matched"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let price: f64 = row["price"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            set_order(state, &token, &side, orig - matched, price);
            to_trade.push(market);
        }
    }
    to_trade
}

