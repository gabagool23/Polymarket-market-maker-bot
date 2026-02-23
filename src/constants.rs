// Minimum position size to trigger position merging (gas cost consideration)
pub const MIN_MERGE_SIZE: f64 = 20.0;

// Stale trade timeout (seconds) before removing from performing
pub const STALE_TRADE_SECS: u64 = 15;

// Update interval for positions/orders (seconds)
pub const UPDATE_INTERVAL_SECS: u64 = 5;

// Market data refresh every N cycles
pub const MARKET_UPDATE_CYCLE: u32 = 6;
