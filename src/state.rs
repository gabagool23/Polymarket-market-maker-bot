use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};

use ordered_float::OrderedFloat;

use crate::types::{MarketRow, OrderState, ParamsMap, Position};

/// Order book side: price -> size (bids: iterate .rev() for best first; asks: normal iteration)
pub type BookSide = BTreeMap<OrderedFloat<f64>, f64>;

#[derive(Debug, Clone)]
pub struct MarketData {
    pub asset_id: String,
    pub bids: BookSide,
    pub asks: BookSide,
}

/// Global shared state (replaces Python global_state module)
pub struct GlobalState {
    pub all_tokens: Vec<String>,
    pub reverse_tokens: HashMap<String, String>,
    pub all_data: HashMap<String, MarketData>,
    pub df: Vec<MarketRow>,
    pub params: ParamsMap,
    pub orders: HashMap<String, OrderState>,
    pub positions: HashMap<String, Position>,
    pub performing: HashMap<String, HashSet<String>>,
    pub performing_timestamps: HashMap<String, HashMap<String, f64>>,
    pub last_trade_update: HashMap<String, f64>,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            all_tokens: Vec::new(),
            reverse_tokens: HashMap::new(),
            all_data: HashMap::new(),
            df: Vec::new(),
            params: ParamsMap::new(),
            orders: HashMap::new(),
            positions: HashMap::new(),
            performing: HashMap::new(),
            performing_timestamps: HashMap::new(),
            last_trade_update: HashMap::new(),
        }
    }
}

pub type SharedState = Arc<RwLock<GlobalState>>;

/// Per-market lock for trading (prevents concurrent perform_trade on same market)
pub type MarketLocks = Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>;
