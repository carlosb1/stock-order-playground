use barter_data::streams::reconnect::Event;
use barter::execution::AccountStreamEvent;
use barter_data::books::{Asks, Bids, OrderBookSide};
use barter_data::event::{DataKind, MarketEvent};
use barter_data::streams::consumer::MarketStreamEvent;
use barter_data::subscription::book::OrderBookEvent;
use barter_execution::{AccountEvent, AccountEventKind};
use barter_execution::order::Order;
use barter_instrument::asset::AssetIndex;
use barter_instrument::exchange::ExchangeIndex;
use barter_instrument::instrument::InstrumentIndex;
use barter_instrument::Side;
use barter_integration::snapshot::Snapshot;
use chrono::{DateTime, Utc};
use futures::SinkExt;
use questdb::ingress::{Buffer, TimestampNanos};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::to_string;
use tracing::Instrument;
use tracing::instrument::WithSubscriber;
use crate::models::BackgroundMessage;

pub struct DBRepository {
    sender: questdb::ingress::Sender,
    buffer:questdb::ingress::Buffer,
    buffer_size: usize
}

impl DBRepository {
    fn maybe_flush(&mut self) {
        if self.buffer_size >= 30 {
            self.sender.flush(&mut self.buffer).expect("Could not flush");
            self.buffer_size = 0;
        }
    }
    pub fn new(db_config: &str) -> Self {
        let sender = questdb::ingress::Sender::from_conf(db_config).expect("It can not initialize my sender");
        let buffer = questdb::ingress::Buffer::new(); //
        DBRepository{
            sender,
            buffer,
            buffer_size: 0
        }
    }
    pub fn write_bg_msg(
        &mut self,
        msg: &BackgroundMessage,
        now_fallback: TimestampNanos,
    ) -> anyhow::Result<()> {
        match msg {
            BackgroundMessage::Account(acc) => {
                use barter_data::streams::reconnect::Event as ReEvent;
                let possible_data = match acc {
                    ReEvent::Item(ac) => {
                        match ac.kind.clone() {
                            AccountEventKind::OrderSnapshot(order) => {
                                Some(extract_account(order))
                            }
                            AccountEventKind::OrderCancelled(cancel) => {
                                None
                            }
                            _ => { None}

                        }
                    }
                    _ => { None}
                };

                let Some((exchange, instrument, quantity, price, evt_kind, time_to_force, side, strategy, cid, ts)) = possible_data else {
                    return Ok(());
                };

                self.buffer
                    .table("account_events")?
                    .column_str("exchange", exchange)?
                    .column_str("instrument", instrument)?
                    .column_str("strategy", strategy)?
                    .column_str("event_kind", evt_kind )?
                    .column_str("time_to_force", time_to_force)?
                    .column_str("side", side)?
                    .column_f64("qty", quantity)?
                    .column_f64("price", price)?
                    .column_str("cid", cid)?
                    .column_str("raw", &to_string(acc)?)?
                    .at(TimestampNanos::new(ts.timestamp_nanos()))?;
                self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
                self.maybe_flush();


            }
            BackgroundMessage::Market(mkt_ev) => {
                use barter_data::streams::reconnect::Event as ReEvent;
                match mkt_ev {
                    ReEvent::Item(ev) => {
                        self.write_market_event(ev, now_fallback)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }


    fn write_market_event(
        &mut self,
        ev: &MarketEvent<InstrumentIndex, DataKind>,
        now_fallback: TimestampNanos,
    ) -> anyhow::Result<()> {
        let ex = ev.exchange.as_str();                // o mapea a &str
        let instrument = format!("{}", ev.instrument.0); // o tu símbolo real

        let ts = ev.time_exchange;
        let ns = ts.timestamp_nanos_opt().expect("valid ts");

        match &ev.kind {
            // Nivel 2 (ej. actualización de un nivel de precio). Ajusta a tu tipo real:
            DataKind::OrderBook(l2) => {
                let (asks, bids, midprice) = match l2 {
                    OrderBookEvent::Snapshot(snapshot) => {
                        let (asks, bids) = extract_book(snapshot.asks(), snapshot.bids());
                        (asks, bids, snapshot.mid_price())
                    }
                    OrderBookEvent::Update(update) => {
                        let (asks, bids) = extract_book(update.asks(), update.bids());
                        (asks, bids, update.mid_price())
                    }
                };
                let asks_json = serde_json::to_string(&asks).unwrap();
                let bids_json = serde_json::to_string(&bids).unwrap();
                let prim_mid_price = if let Some(oval) = midprice {
                    oval.to_f64().unwrap_or(0.0)
                } else {
                    0f64
                };

                self.buffer
                    .table("market_events")?
                    .column_f64("midprice", prim_mid_price)?
                    .column_str("asks", asks_json)?
                    .column_str("bids", bids_json)?
                    .column_str("exchange", ex.to_string())?
                    .column_str("instrument", instrument.as_str())?
                    .column_str("kind", "L2")?
                    //                .column_f64("price_level", price_level)?
                    // .column_f64("quantity", quantity)?
                    .column_str("raw", &to_string(&ev.kind)?)?
                    .at(TimestampNanos::new(ns))?;
                self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
                self.maybe_flush();
            }

            // Trade “simple”
            DataKind::Trade(tr) => {
                let side = if  tr.side == Side::Buy { "bid" } else { "ask" };
                self.buffer
                    .table("market_events")?
                    .column_str("exchange", ex.to_string())?
                    .column_str("instrument", instrument.as_str())?
                    .column_str("kind", "Trade")?
                    .column_str("side", side)?
                    .column_f64("price_level", tr.price)?
                    .column_f64("quantity", tr.amount)?
                    .column_str("raw", &to_string(&ev.kind)?)?
                    .at(TimestampNanos::new(ns))?;
                self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
                self.maybe_flush();
            }

            // Candle, Ticker, etc.: guarda crudo + lo mínimo
            other => {
                self.buffer
                    .table("market_events")?
                    .column_str("exchange", ex.to_string())?
                    .column_str("instrument", instrument.as_str())?
                    .column_str("kind", other.kind_name())?
                    .column_str("raw", &to_string(other)?)?
                    .at(TimestampNanos::new(ns))?;
                self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
                self.maybe_flush();
            }
        }
        Ok(())
    }
}

fn extract_account(acc: Snapshot<Order>) -> (String, String, f64, f64, String, String, String, String, String, DateTime<Utc>)
{
    let exchange = acc.0.key.exchange.to_string();
        let instrument = acc.0.key.instrument.to_string();
        let quantity = acc.0.quantity.to_f64().unwrap_or(-1.0);
        let price = acc.0.price.to_f64().unwrap_or(-1.0);
        let evt_kind = acc.0.kind.to_string();
        let time_to_force = acc.0.time_in_force.to_string();
        let side = acc.0.side.to_string();
        let strategy = acc.0.key.strategy.to_string();
        let cid = acc.0.key.cid.to_string();
        let ts = acc.0.state.time_exchange().unwrap_or_default();
        (exchange, instrument, quantity, price, evt_kind, time_to_force, side, strategy, cid, ts)
    }

fn extract_book(order_asks: &OrderBookSide<Asks>, order_bids: &OrderBookSide<Bids>) -> (Vec<(f32, f32)>, Vec<(f32, f32)>){
    let mut asks: Vec<(f32, f32)> = order_asks.levels().iter().map(|l| (l.price.to_f32().unwrap(), l.amount.to_f32().unwrap())).collect();
    let mut bids: Vec<(f32, f32)> = order_bids.levels().iter().map(|l| (l.price.to_f32().unwrap(), l.amount.to_f32().unwrap())).collect();
    // sort bids descending by price
    bids.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    // sort asks ascending by price
    asks.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    bids.retain(|(_, qty)| *qty != 0. );
    asks.retain(|(_, qty)| *qty != 0.);
    return (asks, bids);
}



fn kind_name(k: &DataKind) -> &'static str {
    match k {
        DataKind::OrderBook(_)     => "L2",
        DataKind::Trade(_)  => "Trade",
        DataKind::Candle(_) => "Candle",
        DataKind::OrderBookL1(_) => "Ticker",
        _ => "Other",
    }
}
