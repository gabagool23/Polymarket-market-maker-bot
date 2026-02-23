//! WebSocket connections to Polymarket: market and user streams.

use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::data_processing::{process_data, process_user_data};
use crate::state::SharedState;
use crate::trading::perform_trade;
use crate::client::PolymarketClient;

const MARKET_WS: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const USER_WS: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/user";

pub async fn connect_market_websocket(
    chunk: Vec<String>,
    state: SharedState,
    client: Arc<PolymarketClient>,
    market_locks: crate::state::MarketLocks,
) {
    loop {
        if let Ok((ws, _)) = connect_async(MARKET_WS).await {
            let (mut write, mut read) = ws.split();
            let msg = serde_json::json!({ "assets_ids": chunk });
            if write.send(Message::Text(msg.to_string())).await.is_err() {
                continue;
            }
            tracing::info!("Sent market subscription: {} assets", chunk.len());
            while let Some(Ok(Message::Text(text))) = read.next().await {
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);
                if let Ok(json) = parsed {
                    let list = if json.is_array() {
                        json.as_array().cloned().unwrap_or_default()
                    } else {
                        vec![json]
                    };
                    let to_trade = {
                        let mut st = state.write().await;
                        process_data(&mut *st, &list, true)
                    };
                    for market in to_trade {
                        let state_c = state.clone();
                        let client_c = client.clone();
                        let locks_c = market_locks.clone();
                        tokio::spawn(async move {
                            perform_trade(state_c, client_c, locks_c, market).await;
                        });
                    }
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

pub async fn connect_user_websocket(
    state: SharedState,
    client: Arc<PolymarketClient>,
    market_locks: crate::state::MarketLocks,
    api_key: String,
    api_secret: String,
    api_passphrase: String,
) {
    let wallet_lower = format!("{:?}", client.browser_wallet).to_lowercase();
    loop {
        if let Ok((ws, _)) = connect_async(USER_WS).await {
            let (mut write, mut read) = ws.split();
            let msg = serde_json::json!({
                "type": "user",
                "auth": {
                    "apiKey": api_key,
                    "secret": api_secret,
                    "passphrase": api_passphrase
                }
            });
            if write.send(Message::Text(msg.to_string())).await.is_err() {
                continue;
            }
            tracing::info!("Sent user subscription");
            while let Some(Ok(Message::Text(text))) = read.next().await {
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);
                if let Ok(json) = parsed {
                    let list = if json.is_array() {
                        json.as_array().cloned().unwrap_or_default()
                    } else {
                        vec![json]
                    };
                    let to_trade = {
                        let mut st = state.write().await;
                        process_user_data(&mut *st, &list, &wallet_lower)
                    };
                    for market in to_trade {
                        let state_c = state.clone();
                        let client_c = client.clone();
                        let locks_c = market_locks.clone();
                        tokio::spawn(async move {
                            perform_trade(state_c, client_c, locks_c, market).await;
                        });
                    }
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}
