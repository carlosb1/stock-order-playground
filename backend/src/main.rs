use std::env;
use std::sync::Arc;

use cbadv::models::websocket::{CandlesEvent, CandleUpdate};
use dotenv::dotenv;
use futures_util::{SinkExt, StreamExt};
use tokio::fs::create_dir_all;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::time::{self, Duration};
use tokio::time::sleep;
use tokio_tungstenite::accept_async;
use tungstenite::Message;

use ws_backend::coinbase::CoinbaseWorker;
use ws_backend::mock::MockWorker;
use ws_backend::Worker;

use crate::binance::BinanceWorker;
use crate::db::inflush_binance_depth;
use crate::ml::run_ml_inter;
use crate::models::BinanceDepthUpdate;

pub mod binance;
pub mod bitcoin;
pub mod coinbase;
mod db;

mod ml;
pub mod mock;
pub mod models;

/// A WebSocket echo server
#[tokio::main]
async fn main() {
    dotenv().ok();
    let host = env::var("WS_SERVER_HOST").unwrap_or("0.0.0.0".to_string());
    let port = env::var("WS_SERVER_PORT").unwrap_or("9001".to_string());
    let questdb_config = env::var("QUESTDB_CONFIG").unwrap_or(
        "http::addr=localhost:9000;username=admin;password=quest;retry_timeout=20000;".to_string(),
    );

    tracing::info!("🌐 WebSocket server running on ws://{}:{}", host, port);

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let server = TcpListener::bind(format!("{host}:{port}")).await.unwrap();
    tracing::info!("Running tasks in parallel...");

    let (multi_tx, _) = broadcast::channel::<String>(16);
    //let mut rx2 = tx.subscribe();

    // running coinbase
    /*
    let product_ids = env::var("PRODUCT_IDS").unwrap_or("BTC-USDC,ETH-USDC".to_string());
    let vec_product_ids = product_ids
    .split(',')
    .map(|s| s.trim().to_string())
    .collect::<Vec<String>>();
     */
    //run_coinbase(vec_product_ids, &multi_tx);
    /* running mocked */
    run_binance(&multi_tx);
    fetch_save_db(&questdb_config, multi_tx.subscribe());
    run_ml(multi_tx.subscribe(), multi_tx.clone());
    /* running mocked */
    //run_mock(&multi_tx);

    tracing::debug!("Starting server");

    loop {
        let (stream, addr) = server.accept().await.unwrap();
        let mut rx_clone = multi_tx.subscribe(); // clone the Arc<Mutex<Receiver>>

        tokio::spawn(async move {
            tracing::info!("New client from: {}", addr);
            let ws_stream = accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws_stream.split();

            tokio::spawn(async move {
                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            tracing::info!("text={:?}", text);
                        }
                        Ok(Message::Binary(bin)) => {
                            tracing::info!("bin={:?}", bin);
                        }
                        Err(e) => {
                            tracing::error!("Error receiving message: {}", e);
                        }
                        _ => {
                            tracing::info!("Received non-text message");
                        }
                    }
                }
            });

            loop {
                //let mut rx_guard = rx_clone.lock().await;
                if let Ok(msg) = rx_clone.recv().await {
                    //let msg = format!("{}",info);

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
}

fn fetch_save_db(questdb_config: &String, mut rx: Receiver<String>) {
    let questdb_config = questdb_config.clone();
    tokio::spawn(async move {
        let mut batch: Vec<String> = Vec::with_capacity(100); // Size queue
        let mut interval = time::interval(Duration::from_millis(500));
        let mut client = questdb::ingress::Sender::from_conf(questdb_config)
            .expect("Failed to create QuestDB client");
        loop {
            tokio::select! {
                Ok(data) = rx.recv() => {
                    batch.push(data);
                    if batch.len() >= 100 {
                        if let Err(e) = inflush_binance_depth(&batch, &mut client).await {
                            eprintln!("Error flushing batch: {e}");
                        }
                        batch.clear();
                    }
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        if let Err(e) = inflush_binance_depth(&batch, &mut client).await {
                            eprintln!("Error flushing batch: {e}");
                        }
                        batch.clear();
                    }
                }
            }
        }
    });
}

fn run_ml(mut rx: Receiver<String>, mut tx: broadcast::Sender<String>) {
    tokio::spawn(async move {
        let mut batch: Vec<String> = Vec::with_capacity(100); // Size queue
        let mut interval = time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                Ok(data) = rx.recv() => {
                    batch.push(data);
                    if batch.len() >= 100 {
                        if let Err(e) = run_ml_inter(&batch, tx.clone()).await {
                            eprintln!("Error flushing batch: {e}");
                        }
                        batch.clear();
                    }
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        if let Err(e) = run_ml_inter(&batch, tx.clone()).await {
                            eprintln!("Error flushing batch: {e}");
                        }
                        batch.clear();
                    }
                }
            }
        }
    });
}

fn run_coinbase(vec_product_ids: Vec<String>, tx: &broadcast::Sender<String>) {
    let cloned_tx = tx.clone();
    let cloned_product_ids = vec_product_ids.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            let mut worker = CoinbaseWorker::new().unwrap();
            worker.products(cloned_product_ids);
            worker.do_work(0, cloned_tx).await;
        });
    });
}

fn run_mock(tx: &broadcast::Sender<String>) {
    tracing::debug!("Starting mock");
    let cloned_tx2 = tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            let mut worker = MockWorker {};
            worker.do_work(1, cloned_tx2).await;
        });
    });
}

fn run_binance(tx: &broadcast::Sender<String>) {
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
}
