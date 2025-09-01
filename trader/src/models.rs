use barter::execution::AccountStreamEvent;
use barter_data::books::Level;
use barter_data::event::DataKind;
use barter_data::streams::consumer::MarketStreamEvent;
use barter_instrument::instrument::InstrumentIndex;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum BackgroundMessage {
    Account(AccountStreamEvent),
    Market(MarketStreamEvent<InstrumentIndex, DataKind>),
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Fill {
    pub price: f64,
    pub quantity: f64,
    pub fee: f64,
    pub side: String,
    pub strategy: String,
    pub ts: DateTime<Utc>,
    pub best_bid: Level,
    pub best_ask: Level,
    pub last_time: DateTime<Utc>,
}