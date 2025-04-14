//! # WebSocket API Example, check out the WebSocket API for all functionality.
//!
//! Shows how to:
//! - Connect WebSocket Client.
//! - Setup Listener and parse messages.
//! - Subscribe to channels.
//! - Unsubscribe to channels.

use tokio::sync::mpsc;
use ws_backend::coinbase::CoinbaseWorker;
use ws_backend::Worker;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    let buffer_queue_window =1000;
    let (tx,  rx) = mpsc::channel::<String>(buffer_queue_window);
    let mut worker = CoinbaseWorker::new().unwrap();
    worker.do_work(1,tx.clone()).await;
}