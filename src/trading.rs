//! Market making logic: perform_trade, send_buy_order, send_sell_order.

use std::sync::Arc;

use polymarket_client_sdk::clob::types::Side;
use rust_decimal::Decimal;
use tokio::sync::RwLock;

use crate::client::PolymarketClient;
use crate::constants::MIN_MERGE_SIZE;
use crate::data_utils::{get_order, get_position, set_position};
use crate::state::{GlobalState, MarketLocks, SharedState};
use crate::trading_utils::{
    get_best_bid_ask_deets, get_buy_sell_amount, get_order_prices, round_down, round_up,
};
use crate::types::{MarketRow, OrderState};

fn round_length(tick_size: f64) -> u32 {
    let s = format!("{:.10}", tick_size);
    if let Some(dot) = s.find('.') {
        let frac = &s[dot + 1..];
        let trim = frac.trim_end_matches('0');
        trim.len() as u32
    } else {
        2
    }
}

pub async fn send_buy_order(
    client: &PolymarketClient,
    state: &mut GlobalState,
    order: &OrderParams,
) {
    let orders = get_order(state, &order.token.to_string());
    let existing_buy_size = orders.buy.size;
    let existing_buy_price = orders.buy.price;

    let price_diff = if existing_buy_price > 0.0 {
        (existing_buy_price - order.price).abs()
    } else {
        f64::INFINITY
    };
    let size_diff = if existing_buy_size > 0.0 {
        (existing_buy_size - order.size).abs()
    } else {
        f64::INFINITY
    };

    let should_cancel = price_diff > 0.005
        || size_diff > order.size * 0.1
        || existing_buy_size == 0.0;

    if should_cancel && (existing_buy_size > 0.0 || orders.sell.size > 0.0) {
        tracing::info!("Cancelling buy orders - price diff: {:.4}, size diff: {:.1}", price_diff, size_diff);
        let _ = client.cancel_all_asset(order.token).await;
    } else if !should_cancel {
        return;
    }

    let incentive_start = order.mid_price - order.max_spread / 100.0;
    let trade = order.price >= incentive_start;

    if trade && order.price >= 0.1 && order.price < 0.9 {
        let neg_risk = order.neg_risk.eq_ignore_ascii_case("TRUE");
        let _ = client
            .create_order(order.token, Side::Buy, order.price, order.size, neg_risk)
            .await;
    }
}

pub async fn send_sell_order(
    client: &PolymarketClient,
    state: &mut GlobalState,
    order: &OrderParams,
) {
    let orders = get_order(state, &order.token.to_string());
    let existing_sell_size = orders.sell.size;
    let existing_sell_price = orders.sell.price;

    let price_diff = if existing_sell_price > 0.0 {
        (existing_sell_price - order.price).abs()
    } else {
        f64::INFINITY
    };
    let size_diff = if existing_sell_size > 0.0 {
        (existing_sell_size - order.size).abs()
    } else {
        f64::INFINITY
    };

    let should_cancel = price_diff > 0.005
        || size_diff > order.size * 0.1
        || existing_sell_size == 0.0;

    if should_cancel && (existing_sell_size > 0.0 || orders.buy.size > 0.0) {
        let _ = client.cancel_all_asset(order.token).await;
    } else if !should_cancel {
        return;
    }

    let neg_risk = order.neg_risk.eq_ignore_ascii_case("TRUE");
    let _ = client
        .create_order(order.token, Side::Sell, order.price, order.size, neg_risk)
        .await;
}

#[derive(Clone)]
pub struct OrderParams {
    pub token: u64,
    pub mid_price: f64,
    pub neg_risk: String,
    pub max_spread: f64,
    pub orders: OrderState,
    pub token_name: String,
    pub row: MarketRow,
    pub price: f64,
    pub size: f64,
}

pub async fn perform_trade(
    state_guard: SharedState,
    client: Arc<PolymarketClient>,
    market_locks: MarketLocks,
    market: String,
) {
    let lock = {
        let mut locks = market_locks.lock().await;
        locks
            .entry(market.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;

    let (row, round_len, params, all_data, reverse_tokens) = {
        let state = state_guard.read().await;
        let row = state.df.iter().find(|r| r.condition_id == market).cloned();
        let (row, rlen, params, data, rev) = match row {
            Some(r) => {
                let rlen = round_length(r.tick_size);
                let params = state.params.get(&r.param_type).cloned();
                let data = state.all_data.get(&market).cloned();
                let rev = state.reverse_tokens.clone();
                (Some(r), rlen, params, data, rev)
            }
            None => (None, 2, None, None, state.reverse_tokens.clone()),
        };
        (row, round_len, params, data, rev)
    };

    let (row, params) = match (row, params) {
        (Some(r), Some(p)) => (r, p),
        _ => return,
    };
    let market_data = match all_data {
        Some(d) => d,
        None => return,
    };

    let deets = [
        (
            "token1",
            row.token1.clone(),
            row.answer1.clone(),
        ),
        (
            "token2",
            row.token2.clone(),
            row.answer2.clone(),
        ),
    ];

    // Position merge
    {
        let mut state = state_guard.write().await;
        let pos_1 = get_position(&*state, &row.token1).size;
        let pos_2 = get_position(&*state, &row.token2).size;
        let amount_to_merge = pos_1.min(pos_2);
        if amount_to_merge > MIN_MERGE_SIZE {
            let (raw_1, _) = client.get_position(row.token1.parse().unwrap_or(0)).await.unwrap_or((0, 0.0));
            let (raw_2, _) = client.get_position(row.token2.parse().unwrap_or(0)).await.unwrap_or((0, 0.0));
            let amount_raw = raw_1.min(raw_2);
            let scaled = amount_raw as f64 / 1e6;
            if scaled > MIN_MERGE_SIZE {
                let neg = row.neg_risk.eq_ignore_ascii_case("TRUE");
                if client.merge_positions(amount_raw, &row.condition_id, neg).await.is_ok() {
                    set_position(&mut *state, &row.token1, "SELL", scaled, 0.0, "merge");
                    set_position(&mut *state, &row.token2, "SELL", scaled, 0.0, "merge");
                }
            }
        }
    }

    for (name, token_str, _answer) in deets {
        let token: u64 = token_str.parse().unwrap_or(0);
        let market_data = {
            let st = state_guard.read().await;
            st.all_data.get(&market).cloned()
        };
        let Some(ref data) = market_data else { continue };

        let mut deets = get_best_bid_ask_deets(data, name, 100.0, 0.1);
        if deets.best_bid.is_none()
            || deets.best_ask.is_none()
            || deets.best_bid_size.is_none()
            || deets.best_ask_size.is_none()
        {
            deets = get_best_bid_ask_deets(data, name, 20.0, 0.1);
        }

        let best_bid = deets.best_bid.map(|p| (p * 10_f64.powi(round_len as i32)).round() / 10_f64.powi(round_len as i32)).unwrap_or(0.0);
        let best_ask = deets.best_ask.map(|p| (p * 10_f64.powi(round_len as i32)).round() / 10_f64.powi(round_len as i32)).unwrap_or(1.0);
        let top_bid = deets.top_bid.unwrap_or(0.0);
        let top_ask = deets.top_ask.unwrap_or(1.0);

        let mut state = state_guard.write().await;
        let pos = get_position(&*state, &token_str);
        let position = round_down(pos.size, 2);
        let avg_price = pos.avg_price;
        let other_token = reverse_tokens.get(&token_str).cloned().unwrap_or_default();
        let other_position = get_position(&*state, &other_token).size;

        let (bid_price, ask_price) = get_order_prices(
            deets.best_bid,
            deets.best_bid_size,
            deets.top_bid,
            deets.best_ask,
            deets.best_ask_size,
            deets.top_ask,
            avg_price,
            &row,
        );
        let bid_price = (bid_price * 10_f64.powi(round_len as i32)).round() / 10_f64.powi(round_len as i32);
        let ask_price = (ask_price * 10_f64.powi(round_len as i32)).round() / 10_f64.powi(round_len as i32);
        let mid_price = (top_bid + top_ask) / 2.0;

        let (buy_amount, sell_amount) = get_buy_sell_amount(position, bid_price, &row, other_position);
        let max_size = row.max_size();

        let order_params = OrderParams {
            token,
            mid_price,
            neg_risk: row.neg_risk.clone(),
            max_spread: row.max_spread,
            orders: get_order(&*state, &token_str),
            token_name: name.to_string(),
            row: row.clone(),
            price: 0.0,
            size: 0.0,
        };
        let orders_snapshot = order_params.orders.clone();
        drop(state);

        let fname = format!("positions/{}.json", market);
        let stop_loss_threshold: f64 = params.get("stop_loss_threshold").and_then(|v| v.as_f64()).unwrap_or(-5.0);
        let spread_threshold: f64 = params.get("spread_threshold").and_then(|v| v.as_f64()).unwrap_or(0.05);
        let volatility_threshold: f64 = params.get("volatility_threshold").and_then(|v| v.as_f64()).unwrap_or(0.5);
        let sleep_period: f64 = params.get("sleep_period").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let take_profit_threshold: f64 = params.get("take_profit_threshold").and_then(|v| v.as_f64()).unwrap_or(5.0);

        if sell_amount > 0.0 && avg_price == 0.0 {
            continue;
        }

        if sell_amount > 0.0 {
            let mut op = order_params.clone();
            op.size = sell_amount;
            op.price = ask_price;
            let state_read = state_guard.read().await;
            let market_data2 = state_read.all_data.get(&market).cloned();
            drop(state_read);
            if let Some(ref nd) = market_data2 {
                let n_deets = get_best_bid_ask_deets(nd, name, 100.0, 0.1);
                let mid = (n_deets.best_bid.unwrap_or(0.0) + n_deets.best_ask.unwrap_or(1.0)) / 2.0;
                let spread = n_deets.best_ask.unwrap_or(1.0) - n_deets.best_bid.unwrap_or(0.0);
                let pnl = (mid - avg_price) / avg_price * 100.0;
                if (pnl < stop_loss_threshold && spread <= spread_threshold) || row.three_hour > volatility_threshold {
                    let mut op_sell = order_params.clone();
                    op_sell.size = sell_amount;
                    op_sell.price = n_deets.best_bid.unwrap_or(0.0);
                    send_sell_order(&*client, &mut *state_guard.write().await, &op_sell).await;
                    let _ = client.cancel_all_market(&market).await;
                    let risk_details = serde_json::json!({
                        "time": chrono::Utc::now().to_rfc3339(),
                        "sleep_till": (chrono::Utc::now() + chrono::Duration::seconds((sleep_period * 3600.0) as i64)).to_rfc3339(),
                        "question": row.question
                    });
                    let _ = std::fs::create_dir_all("positions");
                    let _ = std::fs::write(&fname, risk_details.to_string());
                    continue;
                }
            }
        }

        let mut state = state_guard.write().await;
        if position < max_size && position < 250.0 && buy_amount > 0.0 && buy_amount >= row.min_size {
            let sheet_value = if name == "token2" {
                1.0 - row.best_ask
            } else {
                row.best_bid
            };
            let sheet_value = (sheet_value * 10_f64.powi(round_len as i32)).round() / 10_f64.powi(round_len as i32);
            let mut op = order_params.clone();
            op.size = buy_amount;
            op.price = bid_price;
            let price_change = (op.price - sheet_value).abs();
            let mut send_buy = true;
            if std::path::Path::new(&fname).exists() {
                if let Ok(contents) = std::fs::read_to_string(&fname) {
                    if let Ok(risk) = serde_json::from_str::<serde_json::Value>(&contents) {
                        if let Some(sleep_till) = risk["sleep_till"].as_str() {
                            if let Ok(till) = chrono::DateTime::parse_from_rfc3339(sleep_till) {
                                if chrono::Utc::now() < till {
                                    send_buy = false;
                                }
                            }
                        }
                    }
                }
            }
            if send_buy {
                if row.three_hour > volatility_threshold || price_change >= 0.05 {
                    let _ = client.cancel_all_asset(token).await;
                } else {
                    let rev_pos = get_position(&*state, &reverse_tokens[&token_str]);
                    if rev_pos.size > row.min_size {
                        if orders_snapshot.buy.size > MIN_MERGE_SIZE {
                            let _ = client.cancel_all_asset(token).await;
                        }
                    } else if deets.ask_sum_within_n_percent <= 0.0 {
                        let _ = client.cancel_all_asset(token).await;
                    } else {
                        if deets.best_bid.unwrap_or(0.0) > orders_snapshot.buy.price {
                            send_buy_order(&*client, &mut *state, &op).await;
                        } else if position + orders_snapshot.buy.size < 0.95 * max_size {
                            send_buy_order(&*client, &mut *state, &op).await;
                        } else if orders_snapshot.buy.size > op.size * 1.01 {
                            send_buy_order(&*client, &mut *state, &op).await;
                        }
                    }
                }
            }
        } else if sell_amount > 0.0 {
            let tp_price = round_up(avg_price + (avg_price * take_profit_threshold / 100.0), round_len);
            let order_price = (tp_price).min(ask_price);
            let order_price = round_up(order_price, round_len);
            let orders_now = get_order(&*state, &token_str);
            let diff = (orders_now.sell.price - tp_price).abs() / tp_price * 100.0;
            let mut op = order_params.clone();
            op.size = sell_amount;
            op.price = order_price;
            if diff > 2.0 {
                send_sell_order(&*client, &mut *state, &op).await;
            } else if orders_now.sell.size < position * 0.97 {
                send_sell_order(&*client, &mut *state, &op).await;
            }
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
}
