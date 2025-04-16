use futures_util::StreamExt;
use serde_json::to_string;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::connect_async;
use url::Url;
use crate::db::save_questdb_binance_depth;
use crate::models::{BinanceDepthUpdate, CoinbaseMessage};
use crate::Worker;

pub struct BinanceWorker {
    pub url: Url,
    pub db_config: String,
}
impl BinanceWorker {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("BinanceWorker created");
        let url = Url::parse("wss://stream.binance.com:9443/ws/btcusdt@depth")?;
        let db_config = String::from("http::addr=localhost:9000;username=admin;password=quest;retry_timeout=20000;");
        Ok(BinanceWorker { url, db_config})
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

    async fn do_work(&mut self, id: usize, tx: Sender<String>) {
        let (ws_stream, _) = match connect_async(self.url.as_str()).await {
            Ok((stream, resp)) => {
                tracing::debug!("✅ Connected to  a Binance WebSocket (status: {})\n", resp.status());
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
                        tracing::debug!("📥 Received message: {:?}", msg.clone());
                        if let Ok(update) = serde_json::from_str::<BinanceDepthUpdate>(&msg.to_text().unwrap()) {
                            tracing::debug!("📥 Received message: {:?}", update);
                            let cloned_update = update.clone();
                            /* sending to webscoket */
                            let tx_cloned = tx.clone(); // Clone the sender for the async task
                            tokio::spawn(async move {
                                tx_cloned.send(to_string(&CoinbaseMessage::BinanceDepthUpdate(cloned_update)).unwrap()).await.unwrap(); // Send the candle to the channel
                            });
                            /* save in the db */
                            if let Err(e) = save_questdb_binance_depth(update, &self.db_config) {
                                tracing::error!("❌ Error saving to QuestDB: {}", e);
                            }

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