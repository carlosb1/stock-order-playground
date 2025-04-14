pub mod bitcoin;
pub mod mock;
pub mod coinbase;
pub mod models;

use tokio::net::TcpListener;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tungstenite::{Message};
use std::sync::Arc;
use cbadv::models::websocket::{CandlesEvent, CandleUpdate};
use futures_util::{SinkExt, StreamExt};
use tokio::fs::create_dir_all;
use tokio::time::sleep;
use tokio_tungstenite::accept_async;
use ws_backend::coinbase::CoinbaseWorker;
use ws_backend::mock::MockWorker;
use ws_backend::Worker;

/// A WebSocket echo server
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

        let server = TcpListener::bind("127.0.0.1:9001").await.unwrap();
        tracing::info!("Running tasks in parallel...");

        let buffer_queue_window =1000;
        let (tx,  rx) = mpsc::channel::<String>(buffer_queue_window);
        let rx = Arc::new(tokio::sync::Mutex::new(rx));


        println!("Starting mock");
        /* mocked worker  */
        // lanzar cbadv en hilo separado
        let cloned_tx = tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut worker = CoinbaseWorker::new().unwrap();
                worker.do_work(0, cloned_tx).await;
            });
        });
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

         println!("Starting server");

        loop {
            let (stream, addr) = server.accept().await.unwrap();
            let rx_clone = rx.clone(); // clone the Arc<Mutex<Receiver>>

            tokio::spawn(async move {
                tracing::info!("New client from: {}", addr);
                let ws_stream = accept_async(stream).await.unwrap();
                let (mut write, _) = ws_stream.split();
                let mut count = 0;
                loop {
                    let mut rx_guard = rx_clone.lock().await;
                    if let Some(info) = rx_guard.recv().await {
                        let msg = format!("{} and {}", count, info);
                        if write.send(Message::text(msg)).await.is_err() {
                            tracing::info!("Client {} disconnected", addr);
                            break;
                        }
                        count += 1;
                    } else {
                        break; // channel closed
                    }
                    sleep(Duration::from_millis(100)).await;
                }
            });
        }

        tracing::info!("All tasks completed.");
}