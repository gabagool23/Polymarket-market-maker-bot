//! Polymarket market making bot (Rust).

mod client;
mod constants;
mod data_processing;
mod data_utils;
mod sheets;
mod state;
mod trading;
mod trading_utils;
mod types;
mod websocket;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;

use secrecy::ExposeSecret as _;

use crate::client::PolymarketClient;
use crate::constants::{MARKET_UPDATE_CYCLE, STALE_TRADE_SECS, UPDATE_INTERVAL_SECS};
use crate::data_processing::remove_from_performing;
use crate::data_utils::{update_markets, update_orders, update_positions};
use crate::state::{GlobalState, MarketLocks, SharedState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let client = Arc::new(PolymarketClient::new().await?);
    let state: SharedState = Arc::new(RwLock::new(GlobalState::default()));
    let market_locks: MarketLocks = Arc::new(RwLock::new(std::collections::HashMap::new()));

    let spreadsheet_url = std::env::var("SPREADSHEET_URL").unwrap_or_else(|_| String::new());
    if spreadsheet_url.is_empty() {
        anyhow::bail!("SPREADSHEET_URL not set");
    }

    {
        let mut st = state.write().await;
        update_markets(&mut *st, &spreadsheet_url);
    }
    update_positions(&*client, &mut *state.write().await, false).await;
    update_orders(&*client, &mut *state.write().await).await;

    let df_len = state.read().await.df.len();
    let pos_len = state.read().await.positions.len();
    let ord_len = state.read().await.orders.len();
    tracing::info!(
        "Markets: {}, positions: {}, orders: {}. Starting.",
        df_len,
        pos_len,
        ord_len
    );

    let state_periodic = state.clone();
    let client_periodic = client.clone();
    tokio::spawn(async move {
        let mut i = 1u32;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(UPDATE_INTERVAL_SECS)).await;
            remove_from_pending(&state_periodic).await;
            update_positions(&*client_periodic, &mut *state_periodic.write().await, true).await;
            update_orders(&*client_periodic, &mut *state_periodic.write().await).await;
            if i % MARKET_UPDATE_CYCLE == 0 {
                let url = std::env::var("SPREADSHEET_URL").unwrap_or_default();
                if !url.is_empty() {
                    update_markets(&mut *state_periodic.write().await, &url);
                }
                i = 0;
            }
            i += 1;
        }
    });

    // User WebSocket auth: use API creds from CLOB client (derive or from env)
    let (api_key, api_secret, api_passphrase) = match client.clob.api_keys().await {
        Ok(keys) => (
            keys.api_key.to_string(),
            keys.secret.expose_secret().to_string(),
            keys.passphrase.expose_secret().to_string(),
        ),
        Err(_) => (
            std::env::var("POLYMARKET_API_KEY").unwrap_or_default(),
            std::env::var("POLYMARKET_SECRET").unwrap_or_default(),
            std::env::var("POLYMARKET_PASSPHRASE").unwrap_or_default(),
        ),
    };

    let all_tokens = state.read().await.all_tokens.clone();
    let state_ws_m = state.clone();
    let state_ws_u = state.clone();
    let client_ws_u = client.clone();
    let locks_m = market_locks.clone();
    let locks_u = market_locks.clone();

    tokio::spawn(async move {
        websocket::connect_market_websocket(all_tokens, state_ws_m, client.clone(), locks_m).await;
    });
    tokio::spawn(async move {
        websocket::connect_user_websocket(
            state_ws_u,
            client_ws_u,
            locks_u,
            api_key,
            api_secret,
            api_passphrase,
        )
        .await;
    });

    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

async fn remove_from_pending(state: &SharedState) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let mut st = state.write().await;
    for (col, timestamps) in st.performing_timestamps.clone() {
        for (trade_id, ts) in timestamps {
            if now - ts > STALE_TRADE_SECS as f64 {
                remove_from_performing(&mut *st, &col, &trade_id);
            }
        }
    }
}
