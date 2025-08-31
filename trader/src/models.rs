use barter::execution::AccountStreamEvent;
use barter_data::event::DataKind;
use barter_data::streams::consumer::MarketStreamEvent;
use barter_instrument::instrument::InstrumentIndex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum BackgroundMessage {
    Account(AccountStreamEvent),
    Market(MarketStreamEvent<InstrumentIndex, DataKind>),
}
