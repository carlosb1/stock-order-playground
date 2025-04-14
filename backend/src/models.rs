use std::convert::TryFrom;
use std::fmt;
use cbadv::models::websocket::CandleUpdate;
use serde::{Deserialize, Serialize};
use cbadv::models::websocket::Level2Update;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoinbaseMessage {
    Snapshot(Vec<Candle>),
    Update(Candle),
    Level2(Vec<Level2>),
    Other(String)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub product_id: String,
    pub start: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level2 {
    pub product_id: String,
    pub side: Side,
    pub event_time: String, // or use `DateTime<Utc>` from chrono
    pub price_level: f64,
    pub new_quantity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum Side {
    Bid,
    Ask,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            Side::Bid => "Bid",
            Side::Ask => "Ask",
        };
        write!(f, "{}", s)
    }
}

impl TryFrom<CandleUpdate> for Candle {
    type Error = &'static str;
    fn try_from(candle_update: CandleUpdate) -> Result<Self, Self::Error> {
        Ok(Candle { product_id: candle_update.product_id, start: candle_update.data.start
            , open: candle_update.data.open
            , high: candle_update.data.high
            , low: candle_update.data.low
            , close: candle_update.data.close
            , volume: candle_update.data.volume })
    }
}

impl Level2 {
    pub fn from_with_product_id(product_id: String, level2: Level2Update) -> Result<Self, String> {
        Ok(Level2 { product_id
            , side: if level2.side == cbadv::models::websocket::Level2Side::Bid { Side::Bid } else { Side::Ask }
            , event_time: level2.event_time, price_level: level2.price_level
            , new_quantity: level2.new_quantity })
    }
}