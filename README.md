# Polymarket Market Maker Bot

Rust market making bot for [Polymarket](https://polymarket.com) prediction markets. Real-time order book and user streams, config via Google Sheets, position merging, and configurable market-making logic.

**Repository:** [https://github.com/baker42757/Polymarket-market-maker-bot](https://github.com/baker42757/Polymarket-market-maker-bot)  
**Telegram:** [@baker1119](https://t.me/baker1119)

---

## Requirements

- **Rust** 1.70+ — [rustup.rs](https://rustup.rs)
- **Node.js** — for `poly_merger` (position merge)
- **.env** — `PK`, `BROWSER_ADDRESS`, `SPREADSHEET_URL`

## Quick start

```bash
# 1. Clone
git clone https://github.com/baker42757/Polymarket-market-maker-bot.git
cd Polymarket-market-maker-bot

# 2. Env
cp .env.example .env
# Edit .env: PK, BROWSER_ADDRESS, SPREADSHEET_URL

# 3. Poly merger (for position merge)
cd poly_merger && npm install && cd ..

# 4. Build & run
cargo build --release
cargo run --release
```

## Environment

| Variable | Required | Description |
|----------|----------|-------------|
| `PK` or `POLYMARKET_PRIVATE_KEY` | Yes | Wallet private key |
| `BROWSER_ADDRESS` | Yes | Polymarket (Safe) wallet address |
| `SPREADSHEET_URL` | Yes | Google Sheets URL with Selected Markets, All Markets, Hyperparameters |
| `POLYMARKET_API_KEY` / `SECRET` / `PASSPHRASE` | No | Only if user WebSocket auth fails (fallback) |

## Project layout

| Path | Purpose |
|------|---------|
| `src/` | Rust bot (main, client, trading, websockets, sheets, state) |
| `poly_merger/` | Node script for merging YES/NO positions (used by bot) |
| `Cargo.toml` | Rust deps (polymarket-client-sdk, tokio, etc.) |
| `.env.example` | Env template |

## How it works

- Connects to Polymarket **market** and **user** WebSockets.
- Loads markets and hyperparameters from **Google Sheets** (CSV export, no service account needed for read-only).
- Updates **positions** and **orders** from the API on an interval; cleans stale “performing” trades.
- On book/user events, runs **perform_trade**: merge logic, bid/ask pricing, stop-loss, risk-off, buy/sell sizing and order placement via [polymarket-client-sdk](https://docs.rs/polymarket-client-sdk).
- **Position merge** (recover USDC when holding both YES and NO) is done by `poly_merger/merge.js` (Node).

## License

MIT
