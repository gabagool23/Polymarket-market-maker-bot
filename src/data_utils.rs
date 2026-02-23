//! Data utilities: update positions, orders, markets from API and sheets.

use std::time::SystemTime;

use crate::client::{PolymarketClient, OrderRow, PositionRow};
use crate::sheets;
use crate::state::{GlobalState, SharedState};
use crate::types::{MarketRow, OrderState, OrderSide, Position};

pub fn get_position(state: &GlobalState, token: &str) -> Position {
    state
        .positions
        .get(token)
        .cloned()
        .unwrap_or(Position {
            size: 0.0,
            avg_price: 0.0,
        })
}

pub fn set_position(
    state: &mut GlobalState,
    token: &str,
    side: &str,
    size: f64,
    price: f64,
    _source: &str,
) {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    state.last_trade_update.insert(token.to_string(), now);

    let size_signed = if side.eq_ignore_ascii_case("sell") {
        -size
    } else {
        size
    };

    if let Some(pos) = state.positions.get_mut(token) {
        let prev_price = pos.avg_price;
        let prev_size = pos.size;
        let avg_price_new = if size_signed > 0.0 {
            if prev_size == 0.0 {
                price
            } else {
                (prev_price * prev_size + price * size) / (prev_size + size)
            }
        } else if size_signed < 0.0 {
            prev_price
        } else {
            prev_price
        };
        pos.size += size_signed;
        pos.avg_price = avg_price_new;
    } else {
        state.positions.insert(
            token.to_string(),
            Position {
                size: size_signed,
                avg_price: price,
            },
        );
    }
}

pub fn get_order(state: &GlobalState, token: &str) -> OrderState {
    state.orders.get(token).cloned().unwrap_or(OrderState {
        buy: OrderSide { price: 0.0, size: 0.0 },
        sell: OrderSide { price: 0.0, size: 0.0 },
    })
}

pub fn set_order(state: &mut GlobalState, token: &str, side: &str, size: f64, price: f64) {
    let mut curr = get_order(state, token);
    match side.to_lowercase().as_str() {
        "buy" => curr.buy = OrderSide { price, size },
        _ => curr.sell = OrderSide { price, size },
    }
    state.orders.insert(token.to_string(), curr);
}

pub async fn update_positions(client: &PolymarketClient, state: &mut GlobalState, avg_only: bool) {
    let pos_df = client.get_all_positions().await;
    let Ok(rows) = pos_df else { return };

    for row in rows {
        let asset = row.asset;
        let position = state.positions.entry(asset.clone()).or_insert(Position {
            size: 0.0,
            avg_price: 0.0,
        });
        position.avg_price = row.avg_price;
        if !avg_only {
            position.size = row.size;
        }
    }
}

pub async fn update_orders(client: &PolymarketClient, state: &mut GlobalState) {
    let all = client.get_all_orders().await;
    let Ok(orders_list) = all else { return };

    let mut orders = std::collections::HashMap::new();
    for o in orders_list {
        let token = o.asset_id.clone();
        let entry = orders
            .entry(token)
            .or_insert_with(|| OrderState::default());
        let size_remaining = o.original_size - o.size_matched;
        match o.side.as_str() {
            "BUY" => {
                entry.buy.price = o.price;
                entry.buy.size = size_remaining;
            }
            "SELL" => {
                entry.sell.price = o.price;
                entry.sell.size = size_remaining;
            }
            _ => {}
        }
    }
    state.orders = orders;
}

pub fn update_markets(state: &mut GlobalState, spreadsheet_url: &str) {
    let Ok((received_df, received_params)) = sheets::get_sheet_df(spreadsheet_url) else {
        return;
    };
    if received_df.is_empty() {
        return;
    }
    state.df = received_df;
    state.params = received_params;

    for row in &state.df {
        let t1 = row.token1.clone();
        let t2 = row.token2.clone();
        if !state.all_tokens.contains(&t1) {
            state.all_tokens.push(t1.clone());
        }
        state.reverse_tokens.insert(t1, row.token2.clone());
        state.reverse_tokens.insert(row.token2.clone(), row.token1.clone());
        for col in &[
            format!("{}_buy", t1),
            format!("{}_sell", t1),
            format!("{}_buy", t2),
            format!("{}_sell", t2),
        ] {
            state.performing.entry(col.clone()).or_default();
        }
    }
}
