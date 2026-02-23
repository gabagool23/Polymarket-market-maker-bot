//! Trading helpers: order book analysis, order prices, buy/sell amounts.

use std::collections::BTreeMap;

use ordered_float::OrderedFloat;

use crate::state::MarketData;
use crate::types::{BestBidAsk, MarketRow};

/// Find best price level with at least min_size (and second best, top).
/// reverse=true for bids (iterate high to low), false for asks (low to high).
pub fn find_best_price_with_size(
    book: &BTreeMap<OrderedFloat<f64>, f64>,
    min_size: f64,
    reverse: bool,
) -> (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
) {
    let mut iter = book.iter();
    if reverse {
        iter = iter.rev();
    }
    let mut best_price = None;
    let mut best_size = None;
    let mut second_best_price = None;
    let mut second_best_size = None;
    let mut top_price = None;

    for (price, size) in iter {
        let p = price.0;
        if top_price.is_none() {
            top_price = Some(p);
        }
        if best_price.is_some() {
            second_best_price = Some(p);
            second_best_size = Some(*size);
            break;
        }
        if *size > min_size {
            best_price = Some(p);
            best_size = Some(*size);
        }
    }

    (best_price, best_size, second_best_price, second_best_size, top_price)
}

pub fn get_best_bid_ask_deets(
    market_data: &MarketData,
    name: &str,
    size: f64,
    deviation_threshold: f64,
) -> BestBidAsk {
    let (best_bid, best_bid_size, second_best_bid, second_best_bid_size, top_bid) =
        find_best_price_with_size(&market_data.bids, size, true);
    let (best_ask, best_ask_size, second_best_ask, second_best_ask_size, top_ask) =
        find_best_price_with_size(&market_data.asks, size, false);

    let (mid_price, bid_sum, ask_sum) = match (best_bid, best_ask) {
        (Some(bb), Some(ba)) => {
            let mid = (bb + ba) / 2.0;
            let bid_sum: f64 = market_data
                .bids
                .iter()
                .rev()
                .filter(|(p, _)| *p.0 >= bb && *p.0 <= mid * (1.0 + deviation_threshold))
                .map(|(_, s)| s)
                .sum();
            let ask_sum: f64 = market_data
                .asks
                .iter()
                .filter(|(p, _)| *p.0 >= mid * (1.0 - deviation_threshold) && *p.0 <= ba)
                .map(|(_, s)| s)
                .sum();
            (Some(mid), bid_sum, ask_sum)
        }
        _ => (None, 0.0, 0.0),
    };

    let mut best_bid = best_bid;
    let mut best_bid_size = best_bid_size;
    let mut second_best_bid = second_best_bid;
    let mut second_best_bid_size = second_best_bid_size;
    let mut top_bid = top_bid;
    let mut best_ask = best_ask;
    let mut best_ask_size = best_ask_size;
    let mut second_best_ask = second_best_ask;
    let mut second_best_ask_size = second_best_ask_size;
    let mut top_ask = top_ask;
    let mut bid_sum_within_n_percent = bid_sum;
    let mut ask_sum_within_n_percent = ask_sum;

    if name == "token2" {
        if let (Some(bb), Some(ba), Some(sbb), Some(sba), Some(tb), Some(ta)) = (
            best_bid,
            best_ask,
            second_best_bid,
            second_best_ask,
            top_bid,
            top_ask,
        ) {
            best_bid = Some(1.0 - ba);
            second_best_bid = Some(1.0 - sba);
            top_bid = Some(1.0 - ta);
            best_ask = Some(1.0 - bb);
            second_best_ask = Some(1.0 - sbb);
            top_ask = Some(1.0 - tb);
            std::mem::swap(&mut best_bid_size, &mut best_ask_size);
            std::mem::swap(&mut second_best_bid_size, &mut second_best_ask_size);
            std::mem::swap(&mut bid_sum_within_n_percent, &mut ask_sum_within_n_percent);
        } else if best_bid.is_some() && best_ask.is_some() {
            let bb = best_bid.unwrap();
            let ba = best_ask.unwrap();
            best_bid = Some(1.0 - ba);
            best_ask = Some(1.0 - bb);
            std::mem::swap(&mut best_bid_size, &mut best_ask_size);
            std::mem::swap(&mut bid_sum_within_n_percent, &mut ask_sum_within_n_percent);
        }
    }

    BestBidAsk {
        best_bid,
        best_bid_size,
        second_best_bid,
        second_best_bid_size,
        top_bid,
        best_ask,
        best_ask_size,
        second_best_ask,
        second_best_ask_size,
        top_ask,
        bid_sum_within_n_percent,
        ask_sum_within_n_percent,
    }
}

pub fn get_order_prices(
    best_bid: Option<f64>,
    best_bid_size: Option<f64>,
    top_bid: Option<f64>,
    best_ask: Option<f64>,
    best_ask_size: Option<f64>,
    top_ask: Option<f64>,
    avg_price: f64,
    row: &MarketRow,
) -> (f64, f64) {
    let tick = row.tick_size;
    let min_size = row.min_size;
    let mut bid_price = best_bid.unwrap_or(0.0) + tick;
    let mut ask_price = best_ask.unwrap_or(1.0) - tick;

    if best_bid_size.unwrap_or(0.0) < min_size * 1.5 {
        bid_price = best_bid.unwrap_or(0.0);
    }
    if best_ask_size.unwrap_or(0.0) < 250.0 * 1.5 {
        ask_price = best_ask.unwrap_or(1.0);
    }

    let top_ask_v = top_ask.unwrap_or(1.0);
    let top_bid_v = top_bid.unwrap_or(0.0);
    if bid_price >= top_ask_v {
        bid_price = top_bid_v;
    }
    if ask_price <= top_bid_v {
        ask_price = top_ask_v;
    }
    if (bid_price - ask_price).abs() < 1e-9 {
        bid_price = top_bid_v;
        ask_price = top_ask_v;
    }
    if ask_price <= avg_price && avg_price > 0.0 {
        ask_price = avg_price;
    }
    (bid_price, ask_price)
}

pub fn round_down(number: f64, decimals: u32) -> f64 {
    let factor = 10_f64.powi(decimals as i32);
    (number * factor).floor() / factor
}

pub fn round_up(number: f64, decimals: u32) -> f64 {
    let factor = 10_f64.powi(decimals as i32);
    (number * factor).ceil() / factor
}

pub fn get_buy_sell_amount(
    position: f64,
    _bid_price: f64,
    row: &MarketRow,
    other_token_position: f64,
) -> (f64, f64) {
    let max_size = row.max_size();
    let trade_size = row.trade_size;
    let total_exposure = position + other_token_position;
    let mut buy_amount = 0.0;
    let mut sell_amount = 0.0;

    if position < max_size {
        let remaining_to_max = max_size - position;
        buy_amount = trade_size.min(remaining_to_max);
        if position >= trade_size {
            sell_amount = position.min(trade_size);
        }
    } else {
        sell_amount = position.min(trade_size);
        if total_exposure < max_size * 2.0 {
            buy_amount = trade_size;
        }
    }

    if buy_amount > 0.7 * row.min_size && buy_amount < row.min_size {
        buy_amount = row.min_size;
    }

    if _bid_price < 0.1 && buy_amount > 0.0 {
        if let Ok(m) = row.multiplier.parse::<f64>() {
            if m > 0.0 {
                buy_amount *= m;
            }
        }
    }
    (buy_amount, sell_amount)
}
