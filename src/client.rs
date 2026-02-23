//! Polymarket API client: CLOB (orders, book), Data API (positions), and merge.

use anyhow::Result;
use polymarket_client_sdk::clob::types::{SignatureType, Side};
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::data::Client as DataClient;
use polymarket_client_sdk::data::types::request::PositionsRequest;
use polymarket_client_sdk::types::Decimal;
use polymarket_client_sdk::{derive_safe_wallet, POLYGON, PRIVATE_KEY_VAR};
use std::str::FromStr;

use alloy::primitives::Address;
use alloy::signers::local::LocalSigner;
use alloy::signers::Signer as _;

/// Polymarket client: CLOB (orders) + Data API (positions).
pub struct PolymarketClient {
    pub clob: ClobClient,
    pub data: DataClient,
    pub browser_wallet: alloy::primitives::Address,
    pub signer: LocalSigner,
}

impl PolymarketClient {
    pub async fn new() -> Result<Self> {
        let pk = std::env::var(PRIVATE_KEY_VAR)
            .map_err(|_| anyhow::anyhow!("PK (or POLYMARKET_PRIVATE_KEY) not set"))?;
        let browser_address = std::env::var("BROWSER_ADDRESS")
            .map_err(|_| anyhow::anyhow!("BROWSER_ADDRESS not set"))?;

        let signer = LocalSigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
        let funder = if browser_address.starts_with("0x") {
            browser_address.parse::<Address>().map_err(|e| anyhow::anyhow!("Invalid BROWSER_ADDRESS: {}", e))?
        } else {
            derive_safe_wallet(signer.address(), POLYGON)
        };

        tracing::info!("Initializing Polymarket client...");
        let clob = ClobClient::new("https://clob.polymarket.com", ClobConfig::default())?
            .authentication_builder(&signer)
            .funder(funder)
            .signature_type(SignatureType::GnosisSafe)
            .authenticate()
            .await?;

        let data = DataClient::default();

        Ok(Self {
            browser_wallet: funder,
            clob,
            data,
            signer,
        })
    }

    /// Create and post a limit order.
    pub async fn create_order(
        &self,
        token_id: u64,
        side: Side,
        price: f64,
        size: f64,
        neg_risk: bool,
    ) -> Result<Option<serde_json::Value>> {
        let price_dec = Decimal::from_str(&format!("{:.6}", price))?;
        let size_dec = Decimal::from_str(&format!("{:.6}", size))?;

        let order = self
            .clob
            .limit_order()
            .token_id(token_id.to_string())
            .size(size_dec)
            .price(price_dec)
            .side(side)
            .build()
            .await?;

        let signed = self.clob.sign(&self.signer, order).await?;
        let resp = self.clob.post_order(signed).await;
        match resp {
            Ok(r) => Ok(Some(serde_json::to_value(r)?)),
            Err(e) => {
                tracing::warn!("create_order failed: {}", e);
                Ok(None)
            }
        }
    }

    /// Cancel all orders for an asset.
    pub async fn cancel_all_asset(&self, asset_id: u64) -> Result<()> {
        self.clob.cancel_market_orders(Some(asset_id.to_string()), None).await?;
        Ok(())
    }

    /// Cancel all orders in a market.
    pub async fn cancel_all_market(&self, market_id: &str) -> Result<()> {
        self.clob.cancel_market_orders(None, Some(market_id.to_string())).await?;
        Ok(())
    }

    /// Get open orders (all or filtered).
    pub async fn get_all_orders(&self) -> Result<Vec<OrderRow>> {
        let orders = self.clob.get_orders(None, None).await?;
        let mut rows = Vec::new();
        for o in orders {
            let asset_id = o.asset_id.to_string();
            let side = o.side.to_string();
            let price: f64 = o.price.try_into().unwrap_or(0.0);
            let original_size: f64 = o.original_size.try_into().unwrap_or(0.0);
            let size_matched: f64 = o.size_matched.try_into().unwrap_or(0.0);
            rows.push(OrderRow {
                asset_id,
                side,
                price,
                original_size,
                size_matched,
            });
        }
        Ok(rows)
    }

    /// Get order book for a market (condition id / market).
    pub async fn get_order_book(&self, market: &str) -> Result<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
        let book = self.clob.get_order_book(market).await?;
        let bids: Vec<(f64, f64)> = book
            .bids
            .iter()
            .map(|b| {
                let p: f64 = b.price.try_into().unwrap_or(0.0);
                let s: f64 = b.size.try_into().unwrap_or(0.0);
                (p, s)
            })
            .collect();
        let asks: Vec<(f64, f64)> = book
            .asks
            .iter()
            .map(|a| {
                let p: f64 = a.price.try_into().unwrap_or(0.0);
                let s: f64 = a.size.try_into().unwrap_or(0.0);
                (p, s)
            })
            .collect();
        Ok((bids, asks))
    }

    /// Get all positions for the wallet (Data API).
    pub async fn get_all_positions(&self) -> Result<Vec<PositionRow>> {
        let req = PositionsRequest::builder()
            .user(self.browser_wallet)
            .limit(1000)
            .build()?;
        let positions = self.data.positions(&req).await?;
        let mut rows = Vec::new();
        for p in positions {
            let size: f64 = p.size.try_into().unwrap_or(0.0);
            let avg_price: f64 = p.avg_price.try_into().unwrap_or(0.0);
            rows.push(PositionRow {
                asset: p.asset_id.to_string(),
                size,
                avg_price,
            });
        }
        Ok(rows)
    }

    /// Get raw token balance for a conditional token (on-chain). Uses Data API position if available.
    pub async fn get_position(&self, token_id: u64) -> Result<(u64, f64)> {
        let positions = self.get_all_positions().await?;
        let asset = token_id.to_string();
        for p in positions {
            if p.asset == asset {
                let raw = (p.size * 1_000_000.0) as u64;
                return Ok((raw, p.size));
            }
        }
        Ok((0, 0.0))
    }

    /// Merge positions by calling the existing Node.js poly_merger script (same as Python).
    pub async fn merge_positions(
        &self,
        amount_to_merge: u64,
        condition_id: &str,
        is_neg_risk: bool,
    ) -> Result<String> {
        let args = [
            "node",
            "poly_merger/merge.js",
            &amount_to_merge.to_string(),
            condition_id,
            if is_neg_risk { "true" } else { "false" },
        ];
        let out = tokio::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
            .output()
            .await?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("merge script failed: {}", stderr);
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

#[derive(Debug, Clone)]
pub struct OrderRow {
    pub asset_id: String,
    pub side: String,
    pub price: f64,
    pub original_size: f64,
    pub size_matched: f64,
}

#[derive(Debug, Clone)]
pub struct PositionRow {
    pub asset: String,
    pub size: f64,
    pub avg_price: f64,
}
