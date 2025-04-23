//! # WebSocket API Example, check out the WebSocket API for all functionality.
//!
//! Shows how to:
//! - Connect WebSocket Client.
//! - Setup Listener and parse messages.
//! - Subscribe to channels.
//! - Unsubscribe to channels.

use tokio::sync::broadcast;
use ws_backend::Worker;
use ws_backend::coinbase::CoinbaseWorker;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    let (multi_tx, _) = broadcast::channel::<String>(16);
    let mut worker = CoinbaseWorker::new().unwrap();
    worker.do_work(1, multi_tx.clone()).await;
}
