use barter::engine::Processor;
use rust_decimal::Decimal;
use serde::{Serialize, Deserialize};
use barter_data::event::MarketEvent;
use barter_data::event::DataKind;
use barter::engine::state::instrument::data::InstrumentDataState;
use barter::engine::state::order::in_flight_recorder::InFlightRequestRecorder;
use barter_data::books::{Asks, Bids, OrderBookSide};
use barter_data::subscription::book::OrderBookEvent::{Snapshot, Update};
use barter_execution::{AccountEvent};
use barter_execution::order::request::{OrderRequestCancel, OrderRequestOpen};
use barter_instrument::asset::AssetIndex;
use barter_instrument::exchange::ExchangeIndex;
use barter_instrument::instrument::InstrumentIndex;
use rust_decimal::prelude::Zero;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyAudit {
    pub instrument: InstrumentIndex,
    pub ts_exchange: chrono::DateTime<chrono::Utc>,
    pub ts_received: chrono::DateTime<chrono::Utc>,
    pub best_bid: Decimal,
    pub best_ask: Decimal,
    // opcional: top-N ya agregados
    pub top_bids: Vec<(Decimal, Decimal)>, // (precio, qty)
    pub top_asks: Vec<(Decimal, Decimal)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstrumentMarketDataL2 {
    inner: barter::engine::state::instrument::data::DefaultInstrumentMarketData,
    last_price: Option<Decimal>,
    bids: Vec<(Decimal, Decimal)>,
    asks: Vec<(Decimal, Decimal)>,
    best_bid: Option<(Decimal, Decimal)>,
    best_ask: Option<(Decimal, Decimal)>,
}

impl InstrumentMarketDataL2 {
    pub fn price(&self) -> Option<Decimal> { self.last_price }
    pub fn l2(&self) -> Option<(&[(Decimal, Decimal)], &[(Decimal, Decimal)])> {
        if self.bids.is_empty() || self.asks.is_empty() { return None; }
        Some((&self.bids, &self.asks))
    }


    fn set_from_snapshot(&mut self, asks: &OrderBookSide<Asks>, bids: &OrderBookSide<Bids>) {
        self.asks = asks.levels().iter().map(|l| (l.price, l.amount)).collect();
        self.bids = bids.levels().iter().map(|l| (l.price, l.amount)).collect();
        // Orden "de libro": bids desc, asks asc
        self.bids.sort_unstable_by(|a,b| b.0.cmp(&a.0));
        self.asks.sort_unstable_by(|a,b| a.0.cmp(&b.0));
        // Limpia niveles con amount=0 por si acaso
        self.bids.retain(|(_, qty)| !qty.is_zero());
        self.asks.retain(|(_, qty)| !qty.is_zero());
    }

    fn upsert_level(side: &mut Vec<(Decimal, Decimal)>, price: Decimal, qty: Decimal, is_bids: bool) {
        if qty.is_zero() {
            if let Some(pos) = side.iter().position(|(p, _)| *p == price) {
                side.remove(pos);
            }
        } else {
            if let Some(pos) = side.iter().position(|(p, _)| *p == price) {
                side[pos].1 = qty;
            } else {
                side.push((price, qty));
            }
            if is_bids { side.sort_unstable_by(|a,b| b.0.cmp(&a.0)); }
            else       { side.sort_unstable_by(|a,b| a.0.cmp(&b.0)); }
        }
    }

    fn apply_update(&mut self, asks: &OrderBookSide<Asks>, bids: &OrderBookSide<Bids>) {
        for l in asks.levels().iter() { Self::upsert_level(&mut self.asks, l.price, l.amount, false); }
        for l in bids.levels().iter() { Self::upsert_level(&mut self.bids, l.price, l.amount, true); }
        self.bids.retain(|(_, q)| !q.is_zero());
        self.asks.retain(|(_, q)| !q.is_zero());
    }

    fn build_audit(
        &self,
        instrument: InstrumentIndex,
        ts_exchange: chrono::DateTime<chrono::Utc>,
        ts_received: chrono::DateTime<chrono::Utc>,
        top_n: usize,
    ) -> Option<MyAudit> {
        if self.bids.is_empty() || self.asks.is_empty() { return None; }
        let best_bid = self.bids.first()?.0;
        let best_ask = self.asks.first()?.0;

        Some(MyAudit {
            instrument,
            ts_exchange,
            ts_received,
            best_bid,
            best_ask,
            top_bids: self.bids.iter().copied().take(top_n).collect(),
            top_asks: self.asks.iter().copied().take(top_n).collect(),
        })
    }
}

impl InFlightRequestRecorder<ExchangeIndex, InstrumentIndex>
for InstrumentMarketDataL2
{
    fn record_in_flight_cancel(&mut self, request: &OrderRequestCancel<ExchangeIndex, InstrumentIndex>) {
        self.inner.record_in_flight_cancel(request)
    }

    fn record_in_flight_open(&mut self, request: &OrderRequestOpen<ExchangeIndex, InstrumentIndex>) {
        self.inner.record_in_flight_open(request)
    }
}


impl Processor<&AccountEvent<ExchangeIndex, AssetIndex, InstrumentIndex>> for InstrumentMarketDataL2 {
    type Audit = ();

    fn process(&mut self, evt: &AccountEvent<ExchangeIndex, AssetIndex, InstrumentIndex>) -> Self::Audit {
        self.inner.process(evt);
    }
}

impl Processor<&MarketEvent<InstrumentIndex, DataKind>> for InstrumentMarketDataL2 {
    type Audit = Option<MyAudit>;


    fn process(&mut self, evt: &MarketEvent<InstrumentIndex, DataKind>) -> Self::Audit {
        // primero el default (actualiza price con trades/L1, etc.)
        self.inner.process(evt);
        let instrument = evt.instrument;
        let ts_exchange = evt.time_exchange;
        let ts_received = evt.time_received;

        // luego tu cache L2
        match &evt.kind {
            DataKind::Trade(tr) => {
                // por si quieres priorizar el last trade
                let val = Decimal::from_f64_retain(tr.price).unwrap_or(Decimal::zero());
                self.last_price = Some(val);
                None
            }
            DataKind::OrderBook(Snapshot(snapshot)) => {
                self.set_from_snapshot(snapshot.asks(), snapshot.bids());
                self.build_audit(instrument, ts_exchange, ts_received, 20)
            }
            DataKind::OrderBook(Update(update)) => {
                self.apply_update(update.asks(), update.bids());
                self.build_audit(instrument, ts_exchange, ts_received, 20)

            }
            DataKind::OrderBookL1 (l1)=> {
                let best_bid = l1.best_bid.map(|bid| { (bid.price, bid.amount) });
                let best_ask = l1.best_ask.map(|ask| (ask.price, ask.amount));
                self.best_bid = best_bid;
                self.best_ask = best_ask;
                self.build_audit(instrument, ts_exchange, ts_received, /*top_n=*/1)
            }
            _ => {
                None
            }
        }
    }
}
impl InstrumentDataState<ExchangeIndex, AssetIndex, InstrumentIndex> for InstrumentMarketDataL2 {
    type MarketEventKind = DataKind;
    
    fn price(&self) -> Option<Decimal> {
        self.last_price.or_else(|| self.inner.price())
    }

}
