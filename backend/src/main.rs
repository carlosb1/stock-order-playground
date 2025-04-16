pub mod bitcoin;
pub mod mock;
pub mod coinbase;
pub mod binance;
pub mod models;
mod db;

use std::env;
use tokio::net::TcpListener;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tungstenite::{Message};
use std::sync::Arc;
use cbadv::models::websocket::{CandlesEvent, CandleUpdate};
use dotenv::dotenv;
use futures_util::{SinkExt, StreamExt};
use tokio::fs::create_dir_all;
use tokio::time::sleep;
use tokio_tungstenite::accept_async;
use ws_backend::coinbase::CoinbaseWorker;
use ws_backend::mock::MockWorker;
use ws_backend::Worker;
use crate::binance::BinanceWorker;
use crate::models::BinanceDepthUpdate;

/// A WebSocket echo server
#[tokio::main]
async fn main() {
    dotenv().ok();
    let host = env::var("WS_SERVER_HOST").unwrap_or("0.0.0.0".to_string());
    let port = env::var("WS_SERVER_PORT").unwrap_or("9001".to_string());
    let questdb_config = env::var("QUESTDB_CONFIG").unwrap_or("http::addr=localhost:9000;username=admin;password=quest;retry_timeout=20000;".to_string());
    let product_ids = env::var("PRODUCT_IDS").unwrap_or("BTC-USDC,ETH-USDC".to_string());

    let vec_product_ids = product_ids
        .split(',')
        .map(|s| s.trim().to_string())
        .collect::<Vec<String>>();

    tracing::info!("🌐 WebSocket server running on ws://{}:{}", host, port);

    tracing_subscriber::fmt()
        .init();

        let server = TcpListener::bind(format!("{host}:{port}")).await.unwrap();
        tracing::info!("Running tasks in parallel...");

        let buffer_queue_window =1000;
        let (tx,  rx) = mpsc::channel::<String>(buffer_queue_window);
        let rx = Arc::new(tokio::sync::Mutex::new(rx));
        /* mocked worker  */
        // running coinbase

    /*
        let cloned_tx = tx.clone();
        let cloned_questdb_config = questdb_config.clone();
        let cloned_product_ids = vec_product_ids.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut worker = CoinbaseWorker::new().unwrap();
                worker.db_config(cloned_questdb_config);
                worker.products(cloned_product_ids);
                worker.do_work(0, cloned_tx).await;
            });
        });
*/
        /* running mocked */
        tracing::debug!("Starting binance");
        let cloned_tx3 = tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut worker = BinanceWorker::new().unwrap();
                worker.do_work(1, cloned_tx3).await;
            });
        });

        /* running mocked */
    /*
        tracing::debug!("Starting mock");
        let cloned_tx2 = tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut worker = MockWorker{};
                worker.do_work(1, cloned_tx2).await;
            });
        });
*/
         tracing::debug!("Starting server");

        loop {
            let (stream, addr) = server.accept().await.unwrap();
            let rx_clone = rx.clone(); // clone the Arc<Mutex<Receiver>>

            tokio::spawn(async move {
                tracing::info!("New client from: {}", addr);
                let ws_stream = accept_async(stream).await.unwrap();
                let (mut write, _) = ws_stream.split();
                loop {
                    let mut rx_guard = rx_clone.lock().await;
                    if let Some(info) = rx_guard.recv().await {
                        let msg = format!("{}",info);
                        if write.send(Message::text(msg)).await.is_err() {
                            tracing::info!("Client {} disconnected", addr);
                            break;
                        }
                    } else {
                        break; // channel closed
                    }
                    sleep(Duration::from_millis(100)).await;
                }
            });
        }

        tracing::info!("All tasks completed.");
}
