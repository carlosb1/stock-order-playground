use barter_data::books::{Asks, Bids, OrderBookSide};
use barter_execution::order::Order;
use barter_execution::order::request::OrderResponseCancel;
use barter_integration::snapshot::Snapshot;
use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

pub fn extract_order_snapshot(acc: Snapshot<Order>) -> (String, String, f64, f64, String, String, String, String, DateTime<Utc>)
{
    let exchange = acc.0.key.exchange.to_string();
    let instrument = acc.0.key.instrument.to_string();
    let quantity = acc.0.quantity.to_f64().unwrap_or(-1.0);
    let price = acc.0.price.to_f64().unwrap_or(-1.0);
    let evt_kind = acc.0.kind.to_string();
    let time_to_force = acc.0.time_in_force.to_string();
    let side = acc.0.side.to_string();
    let strategy = acc.0.key.strategy.to_string();
    let ts = acc.0.state.time_exchange().unwrap_or_default();
    (exchange, instrument, quantity, price, evt_kind, time_to_force, side, strategy, ts)
}

pub fn extract_order_cancel(cancel: OrderResponseCancel) -> (String, String, String, DateTime<Utc>)
{
    let ex = cancel.key.exchange.to_string();
    let instrument = cancel.key.instrument.to_string();
    let strategy = cancel.key.strategy.to_string();
    let ts = if let Ok(state) = cancel.state {
        state.time_exchange
    } else {
        DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc)
    };
    (ex, instrument, strategy, ts)
}

pub fn extract_book_l2(order_asks: &OrderBookSide<Asks>, order_bids: &OrderBookSide<Bids>) -> (Vec<(f32, f32)>, Vec<(f32, f32)>){
    let mut asks: Vec<(f32, f32)> = order_asks.levels().iter().map(|l| (l.price.to_f32().unwrap(), l.amount.to_f32().unwrap())).collect();
    let mut bids: Vec<(f32, f32)> = order_bids.levels().iter().map(|l| (l.price.to_f32().unwrap(), l.amount.to_f32().unwrap())).collect();
    // sort bids descending by price
    bids.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    // sort asks ascending by price
    asks.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    bids.retain(|(_, qty)| *qty != 0. );
    asks.retain(|(_, qty)| *qty != 0.);
    (asks, bids)
}


pub fn create_book_snapshot(asks: &OrderBookSide<Asks>, bids: &OrderBookSide<Bids>) -> (Vec<(Decimal, Decimal)>, Vec<(Decimal, Decimal)>) {
    let mut asks: Vec<(Decimal, Decimal)> = asks.levels().iter().map(|l| (l.price, l.amount)).collect();
    let mut bids: Vec<(Decimal, Decimal)> = bids.levels().iter().map(|l| (l.price, l.amount)).collect();
    // Orden "de libro": bids desc, asks asc
    bids.sort_unstable_by(|a,b| b.0.cmp(&a.0));
    asks.sort_unstable_by(|a,b| a.0.cmp(&b.0));
    // Limpia niveles con amount=0 por si acaso
    bids.retain(|(_, qty)| !qty.is_zero());
    asks.retain(|(_, qty)| !qty.is_zero());
    return (asks, bids);
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

pub fn apply_update(original_book:(Vec<(Decimal, Decimal)>, Vec<(Decimal, Decimal)>),  asks: &OrderBookSide<Asks>, bids: &OrderBookSide<Bids>) -> (Vec<(Decimal, Decimal)>, Vec<(Decimal, Decimal)>) {
    let (mut original_asks, mut original_bids) = original_book;
    for l in asks.levels().iter() { upsert_level(&mut original_asks, l.price, l.amount, false); }
    for l in bids.levels().iter() { upsert_level(&mut original_bids, l.price, l.amount, true); }
    original_bids.retain(|(_, q)| !q.is_zero());
    original_asks.retain(|(_, q)| !q.is_zero());
    (original_asks, original_bids)

}