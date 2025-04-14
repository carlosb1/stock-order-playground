use std::process::exit;
use std::time::{Duration, Instant};
use cbadv::{WebSocketClient, WebSocketClientBuilder};
use cbadv::models::websocket::{CandleUpdate, Channel, EndpointStream, Events, EventType, Level2Event, Message};
use cbadv::types::CbResult;
use chrono::{DateTime, Utc};
use chrono::format::Numeric::Timestamp;
use lazy_static::lazy_static;
use serde_json::to_string;
use tokio::sync::mpsc::Sender;
use crate::models::{Candle, CoinbaseMessage, Level2, Side};
use super::{models, Worker};

pub struct CoinbaseWorker {
    client: WebSocketClient,
    products: Vec<String>,
    db_config: String
}

lazy_static! {
    pub static ref DEFAULT_PRODUCTS: Vec<String> = vec![
        "BTC-USDC".to_string(),
        "ETH-USDC".to_string()
    ];
}
impl CoinbaseWorker {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("CoinbaseWorker created");
        let mut client = WebSocketClientBuilder::new()
            .auto_reconnect(true)
            .max_retries(20)
            .build()?;
        let products = DEFAULT_PRODUCTS.clone();
        let db_config = String::from("http::addr=localhost:9000;username=admin;password=quest;retry_timeout=20000;");
        Ok(CoinbaseWorker{client, products, db_config})
    }
    pub fn db_config(&mut self, db_config: String) {
        self.db_config = db_config
    }

    pub fn products(&mut self, products: Vec<String>) {
        self.products = products
    }
}

async fn message_action(msg: CbResult<Message>, tx: Sender<String>, db_config: String) -> anyhow::Result<()> {
     match msg {
        Ok(Message {
               channel,
               client_id,
               timestamp,
               sequence_num,
               events: Events::Candles(candles_events),
           }) => {
            tracing::debug!("channel={:?}, client_id={:?}, timestamp={:?}, sequence_num={:?}"
                    ,channel
                    ,client_id
                    ,timestamp
                    ,sequence_num);
            for ticker in candles_events {
                let typ = ticker.r#type;
                tracing::debug!("typ={:?}", typ);

                let mut candle_updates = Vec::<Candle>::new();
                for candle in ticker.candles {
                    if let Ok(candle) = Candle::try_from(candle) {
                        candle_updates.push(candle);
                    }
                }
                /* send thread */
                let candled_cloneds  = candle_updates.clone();
                let tx_cloned = tx.clone(); // Clone the sender for the async task
                if typ == EventType::Snapshot {
                    tokio::spawn(async move {
                        tx_cloned.send(to_string(&CoinbaseMessage::Snapshot(candled_cloneds)).unwrap()).await.unwrap(); // Send the candle to the channel
                    });
                } else if typ == EventType::Update {
                    let candle_cloned = candle_updates.first().unwrap().clone();
                    tokio::spawn(async move {
                        tx_cloned.send(to_string(&CoinbaseMessage::Update(candle_cloned)).unwrap()).await.unwrap(); // Send the candle to the channel
                    });
                } else {
                    tracing::error!("Unknown event type: {:?}", typ);
                }

                /*  save to db */
                for candle in candle_updates.iter() {
                    save_questdb_candle(candle.clone(), &db_config)?;
                }
            }
        }
        Ok(Message {
               channel,
               client_id,
               timestamp,
               sequence_num,
               events: Events::Level2(level2_events),
           }) =>
            {
                tracing::debug!("channel={:?}, client_id={:?}, timestamp={:?}, sequence_num={:?}"
                    ,channel
                    ,client_id
                    ,timestamp
                    ,sequence_num);
                let mut events_to_send: Vec<Level2> = Vec::new();
                for level2_event in level2_events {
                    let typ = level2_event.r#type;
                    tracing::debug!("typ={:?}", typ);
                    let product_id = level2_event.product_id;
                    for event in level2_event.updates {
                        if let Ok(parsed_event) = Level2::from_with_product_id(product_id.clone(), event) {
                            events_to_send.push(parsed_event);
                        }
                    }
                }
                /* send events */
                let events_cloned = events_to_send.clone();
                let tx_cloned = tx.clone(); // Clone the sender for the async task
                tokio::spawn(async move {
                    let msg_to_send = to_string(&CoinbaseMessage::Level2(events_cloned)).unwrap();
                    tx_cloned.send(msg_to_send.clone()).await.unwrap(); // Send the candle to the channel
                });

                /*  save db */
                for event in events_to_send.iter() {
                    save_questdb_event(event.clone(), &db_config)?;
                }

            }, // Leverage Debug for all Message variants
        Ok(message) => {
            if let Ok(str_message) = to_string(&message) {
                /* send message */
                let tx_cloned = tx.clone(); // Clone the sender for the async task
                tokio::spawn(async move {
                    tracing::debug!("msg_to_send={:?}", str_message.clone());
                    tx_cloned.send(to_string(&CoinbaseMessage::Other(str_message.clone())).unwrap()).await.unwrap(); // Send the candle to the channel
                });
            }
            tracing::debug!("Received message: {:?}", message);
        },
        Err(error) => tracing::error!("Error: {error}"), // Handle WebSocket errors
    };

    Ok(())
}

fn save_questdb_candle(candle: Candle, db_config: &String) -> anyhow::Result<()> {
    let mut sender = questdb::ingress::Sender::from_conf(
        db_config.clone()
    )?;
    /* Save struct */
    let mut buffer = questdb::ingress::Buffer::new();
    buffer
        .table("candles")?
        .symbol("product_id", candle.product_id)?
        .column_f64("open", candle.open)?
        .column_f64("high", candle.high)?
        .column_f64("low", candle.low)?
        .column_f64("close", candle.close)?
        .column_f64("volume", candle.volume)?
        .column_ts("process_time", questdb::ingress::TimestampNanos::now())?
        .at(questdb::ingress::TimestampNanos::new(candle.start as i64))?;
    sender.flush(&mut buffer)?;
    Ok(())
}

fn save_questdb_event(level2_event: Level2, db_config: &String) -> anyhow::Result<()> {
    /* Save struct */
    println!("level={:?}", level2_event);

    let mut sender = questdb::ingress::Sender::from_conf(
        db_config.clone()
    )?;
    let mut buffer = questdb::ingress::Buffer::new();

    let dt: DateTime<Utc> = level2_event.event_time.parse().expect("Failed to parse datetime");
    buffer
        .table("events")?
        .symbol("product_id", level2_event.product_id)?
        .column_str("side", (
            if level2_event.side == Side::Bid {""} else {""}).to_string()
        )?
        .column_f64("price_level", level2_event.price_level)?
        .column_f64("new_quantity", level2_event.new_quantity)?
        .column_ts("process_time", questdb::ingress::TimestampNanos::now())?
        .at(questdb::ingress::TimestampNanos::new(dt.timestamp_nanos_opt().unwrap()))?;


    sender.flush(&mut buffer)?;
    Ok(())
}

#[async_trait::async_trait(?Send)]
impl Worker for CoinbaseWorker {
    async fn do_work(&mut self, id: usize, tx_cloned: Sender<String>) {
        tracing::info!("Task {} starting...", id);

        let readers = self.client
            .connect()
            .await
            .expect("Could not connect to WebSocket");

        // Heartbeats is a great way to keep a connection alive and not timeout.
        self.client.subscribe(&Channel::Heartbeats, &[]).await.unwrap();

        // Get updates (subscribe) on products and currencies.
        self.client
            .subscribe(&Channel::Candles, &self.products)
            .await
            .unwrap();

        // Get updates (subscribe) on products and currencies.
        self.client.subscribe(&Channel::Level2, &self.products).await.unwrap();

        // Stop obtaining (unsubscribe) updates on products and currencies.
        self.client
            .unsubscribe(&Channel::Status, &self.products)
            .await
            .unwrap();
        const TICK_RATE: u64 = 1000 / 60;
        let mut last_tick = Instant::now();
        let mut stream: EndpointStream = readers.into();

        loop {
            // Fetch messages from the WebSocket stream.
            let _ = self.client.fetch_async(&mut stream, 100, |msg| async {
                message_action(msg, tx_cloned.clone(), self.db_config.clone()).await.map_err(|e| e.to_string())
            }).await;

            // Calculate the time since the last tick and sleep for the remaining time to hit the tick rate.
            let last_tick_ms = last_tick.elapsed().as_millis();
            let timeout = match u64::try_from(last_tick_ms) {
                Ok(ms) => TICK_RATE.saturating_sub(ms),
                Err(why) => {
                    tracing::error!("Conversion error: {why}");
                    TICK_RATE
                }
            };

            // Sleep for the remaining time to hit the tick rate. Prevent busy loop.
            tokio::time::sleep(Duration::from_millis(timeout)).await;
            last_tick = Instant::now();
        }
    }

}