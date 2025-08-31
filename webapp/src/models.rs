use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
pub struct Msg { pub Market: Market }

#[derive(Deserialize, Debug, Clone)]
pub struct Market { pub Item: Item }

#[derive(Deserialize, Debug, Clone)]
pub struct Item { pub kind: Kind }

/// "kind": { "OrderBookL1": {...} } | { "OrderBook": { "Update": {...} | "Snapshot": {...} } } | { "Trade": {...} }
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub enum Kind {
    OrderBookL1(L1),
    OrderBook(OrderBookVariant),
    Trade(Trade),
}

#[derive(Deserialize, Debug, Clone)]
pub struct L1 {
    #[serde(default)] pub best_bid: Level,
    #[serde(default)] pub best_ask: Level,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Level {
    #[serde(default)] pub price: String,
    #[serde(default)] pub amount: String,
}

/// "OrderBook": { "Update": {...} } o "OrderBook": { "Snapshot": {...} }
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub enum OrderBookVariant {
    Update(OrderBookUpdate),
    Snapshot(OrderBookSnapshot),
}

#[derive(Deserialize, Debug, Clone)]
pub struct OrderBookUpdate {
    #[serde(default)] pub bids: SideLevels,
    #[serde(default)] pub asks: SideLevels,
    // otros campos (sequence, time_engine, etc.) se ignoran
}

#[derive(Deserialize, Debug, Clone)]
pub struct OrderBookSnapshot {
    #[serde(default)] pub bids: SideLevels,
    #[serde(default)] pub asks: SideLevels,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct SideLevels {
    #[serde(default)] pub levels: Vec<Level>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Trade {
    #[serde(default)] pub id: String,
    #[serde(default)] pub price: f64,
    #[serde(default)] pub amount: f64,
    #[serde(default)] pub side: String,
}

#[derive(Default, Clone)]
pub struct Book {
    bids: BTreeMap<OrderedFloat<f64>, f64>,
    asks: BTreeMap<OrderedFloat<f64>, f64>,
}

use ordered_float::OrderedFloat;
impl Book {
    pub fn apply(&mut self, up: &OrderBookUpdate) {
        for l in &up.bids.levels {
            let p = OrderedFloat(l.price.parse::<f64>().unwrap_or(0.0));
            let q = l.amount.parse::<f64>().unwrap_or(0.0);
            if q <= 0.0 {
                self.bids.remove(&p);
            } else {
                self.bids.insert(p, q);
            }
        }
        for l in &up.asks.levels {
            let p = OrderedFloat(l.price.parse::<f64>().unwrap_or(0.0));
            let q = l.amount.parse::<f64>().unwrap_or(0.0);
            if q <= 0.0 {
                self.asks.remove(&p);
            } else {
                self.asks.insert(p, q);
            }
        }
    }

    /// Devuelve (xs, y_bids, y_asks) donde xs es la unión ordenada de precios
    /// y y_* son acumulados "step-like". `max_points` limita profundidad por lado.
    pub fn to_depth_union(&self, max_points: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        // --- 1) Acumulados por lado ---
        // bids: mayor->menor
        let mut bids: Vec<(f64, f64)> = self.bids.iter().map(|(p,q)| (p.0, *q)).collect();
        bids.sort_by(|a,b| b.0.partial_cmp(&a.0).unwrap());
        if bids.len() > max_points { bids.truncate(max_points); }
        let mut acc = 0.0;
        for b in &mut bids { acc += b.1.max(0.0); b.1 = acc; }

        // asks: menor->mayor
        let mut asks: Vec<(f64, f64)> = self.asks.iter().map(|(p,q)| (p.0, *q)).collect();
        asks.sort_by(|a,b| a.0.partial_cmp(&b.0).unwrap());
        if asks.len() > max_points { asks.truncate(max_points); }
        let mut acc2 = 0.0;
        for a in &mut asks { acc2 += a.1.max(0.0); a.1 = acc2; }

        // --- 2) Unión de X (precios) ---
        let mut xs: Vec<f64> = bids.iter().map(|x| x.0)
            .chain(asks.iter().map(|x| x.0)).collect();
        xs.sort_by(|a,b| a.partial_cmp(b).unwrap());
        xs.dedup_by(|a,b| (*a - *b).abs() < 1e-9);

        // --- 3) Lookup acumulado por precio (para step) ---
        let map_b: BTreeMap<OrderedFloat<f64>, f64> = bids.into_iter()
            .map(|(p,v)| (OrderedFloat(p), v)).collect();
        let map_a: BTreeMap<OrderedFloat<f64>, f64> = asks.into_iter()
            .map(|(p,v)| (OrderedFloat(p), v)).collect();

        let mut last_b = f64::NAN;
        let mut last_a = f64::NAN;
        let mut y_b = Vec::with_capacity(xs.len());
        let mut y_a = Vec::with_capacity(xs.len());

        for &x in &xs {
            if let Some(v) = map_b.get(&OrderedFloat(x)) { last_b = *v; }
            if let Some(v) = map_a.get(&OrderedFloat(x)) { last_a = *v; }
            y_b.push(last_b);
            y_a.push(last_a);
        }

        (xs, y_b, y_a)
    }
}