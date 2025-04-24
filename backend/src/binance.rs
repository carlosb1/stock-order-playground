use futures_util::StreamExt;
use serde_json::to_string;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use url::Url;

use crate::models::{AppMessage, BinanceDepthUpdate};
use crate::Worker;

pub const BTC_USDT: &str = "btcusdt@depth";

pub struct BinanceWorker {
    pub url: Url,
    pub db_config: String,
}
impl BinanceWorker {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("BinanceWorker created");
        let url =
            Url::parse(format!("wss://stream.binance.com:9443/ws/{}", BTC_USDT).as_mut_str())?;
        let db_config = String::from(
            "http::addr=localhost:9000;username=admin;password=quest;retry_timeout=20000;",
        );
        Ok(BinanceWorker { url, db_config })
    }

    pub fn endpoint(&mut self, endpoint: String) -> anyhow::Result<()> {
        let url_str = format!("wss://stream.binance.com:9443/ws/{}", endpoint);
        self.url = Url::parse(url_str.as_str())?;
        Ok(())
    }
    pub fn url(&mut self, url: String) -> anyhow::Result<()> {
        self.url = Url::parse(url.as_str())?;
        Ok(())
    }
    pub fn db_config(&mut self, db_config: String) {
        self.db_config = db_config
    }
}

#[async_trait::async_trait(?Send)]
impl Worker for BinanceWorker {
    async fn do_work(&mut self, id: usize, tx: broadcast::Sender<String>) {
        tracing::debug!("Task {} starting...", id);
        let (ws_stream, _) = match connect_async(self.url.as_str()).await {
            Ok((stream, resp)) => {
                tracing::debug!(
                    "✅ Connected to  a Binance WebSocket (status: {})\n",
                    resp.status()
                );
                (stream, resp)
            }
            Err(e) => {
                tracing::error!("❌ Error connecting to the websocket: {}", e);
                return;
            }
        };
        let (_, mut read) = ws_stream.split();
        while let Some(msg) = read.next().await {
            match msg {
                Ok(msg) => {
                    if msg.is_text() {
                        //tracing::debug!("📥 Received message: {:?}", msg.clone());

                        if let Ok(update) =
                            serde_json::from_str::<BinanceDepthUpdate>(&msg.to_text().unwrap())
                        {
                            let cloned_update = update.clone();
                            tx.send(
                                to_string(&AppMessage::BinanceDepthUpdate(cloned_update)).unwrap(),
                            )
                            .unwrap(); // Send the candle to the channel
                        } else {
                            tracing::error!("❌ Error converting message to text");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Error de WebSocket: {}", e);
                    break;
                }
            }
        }
    }
}
