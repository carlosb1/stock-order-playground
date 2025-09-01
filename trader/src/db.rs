
use barter_data::books::{Asks, Bids, OrderBookSide};
use barter_data::event::{DataKind};
use barter_execution::order::Order;
use barter_execution::order::request::OrderResponseCancel;
use barter_integration::snapshot::Snapshot;
use chrono::{DateTime, NaiveDateTime, Utc};
use questdb::ingress::{TimestampNanos};
use rust_decimal::prelude::ToPrimitive;
use serde_json::to_string;



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
        DBRepository {
            sender,
            buffer,
            buffer_size: 0
        }
    }
    pub fn db_insert_order_account(
        &mut self,
        ns: i64,
        exchange: &str,
        instrument: &str,
        quantity: f64,
        price: f64,
        evt_kind: &str,
        time_to_force: &str,
        side: &str,
        strategy: &str
    ) -> anyhow::Result<()> {
        self.buffer
            .table("account_events")?
            .column_str("type", "snapshot")?
            .column_str("exchange", exchange)?
            .column_str("instrument", instrument)?
            .column_str("strategy", strategy)?
            .column_str("event_kind", evt_kind)?
            .column_str("time_to_force", time_to_force)?
            .column_str("side", side)?
            .column_f64("qty", quantity)?
            .column_f64("price", price)?
            .at(TimestampNanos::new(ns))?;
        self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
        self.maybe_flush();

        Ok(())
    }

    pub fn db_insert_market_other(&mut self, ns: i64, ex: &str, instrument: &str, kind_name: &str, other: &DataKind) -> anyhow::Result<()>
    {
        self.buffer
            .table("market_events")?
            .column_str("exchange", ex.to_string())?
            .column_str("instrument", instrument)?
            .column_str("kind", kind_name)?
            .column_str("raw", &to_string(other)?)?
            .at(TimestampNanos::new(ns))?;
        self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
        self.maybe_flush();
        Ok(())
    }

    pub fn db_insert_order_book(&mut self, ns: i64, ex: &str, instrument: &str, prim_mid_price: f64,
                                asks_json: &str, bids_json: &str, kind: &DataKind) -> anyhow::Result<()> {
        self.buffer
            .table("market_events")?
            .column_f64("midprice", prim_mid_price)?
            .column_str("asks", asks_json)?
            .column_str("bids", bids_json)?
            .column_str("exchange", ex.to_string())?
            .column_str("instrument", instrument)?
            .column_str("kind", "L2")?
            //                .column_f64("price_level", price_level)?
            // .column_f64("quantity", quantity)?
            .column_str("raw", &to_string(&kind)?)?
            .at(TimestampNanos::new(ns))?;
        self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
        self.maybe_flush();
        Ok(())
    }

    pub fn db_insert_order_cancel(&mut self, ns: i64, ex: &str, instrument: &str, strategy: &str) -> anyhow::Result<()> {
        self.buffer
            .table("account_events")?
            .column_str("type", "cancel")?
            .column_str("exchange", ex)?
            .column_str("instrument", instrument)?
            .column_str("strategy", strategy)?
            .at(TimestampNanos::new(ns))?;
        self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
        self.maybe_flush();
        Ok(())
    }

    pub fn db_insert_trade(&mut self, ns: i64, ex: &str, instrument: &str, side: &str, price: f64, amount: f64, kind: &DataKind) -> anyhow::Result<()> {
        self.buffer
            .table("market_events")?
            .column_str("exchange", ex)?
            .column_str("instrument", instrument)?
            .column_str("kind", "Trade")?
            .column_str("side", side)?
            .column_f64("price_level", price)?
            .column_f64("quantity", amount)?
            .column_str("raw", &to_string(&kind)?)?
            .at(TimestampNanos::new(ns))?;
        self.buffer_size += 1;          // ← solo aquí, tras escribir una fila
        self.maybe_flush();
        Ok(())
    }
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
