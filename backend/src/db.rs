use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{BinanceDepthUpdate, BinanceMessage, Candle, Level2, Side};

pub fn save_questdb_candle(candle: Candle, db_config: &String) -> anyhow::Result<()> {
    let mut sender = questdb::ingress::Sender::from_conf(db_config.clone())?;
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

pub fn save_questdb_event(level2_event: Level2, db_config: &String) -> anyhow::Result<()> {
    /* Save struct */
    tracing::debug!("level={:?}", level2_event);

    let mut sender = questdb::ingress::Sender::from_conf(db_config.clone())?;
    let mut buffer = questdb::ingress::Buffer::new();

    let dt: DateTime<Utc> = level2_event
        .event_time
        .parse()
        .expect("Failed to parse datetime");
    buffer
        .table("events")?
        .symbol("product_id", level2_event.product_id)?
        .column_str(
            "side",
            (if level2_event.side == Side::Bid {
                ""
            } else {
                ""
            })
            .to_string(),
        )?
        .column_f64("price_level", level2_event.price_level)?
        .column_f64("new_quantity", level2_event.new_quantity)?
        .column_ts("process_time", questdb::ingress::TimestampNanos::now())?
        .at(questdb::ingress::TimestampNanos::new(
            dt.timestamp_nanos_opt().unwrap(),
        ))?;

    sender.flush(&mut buffer)?;
    Ok(())
}
pub async fn inflush_binance_depth(
    updates: &Vec<String>,
    sender: &mut questdb::ingress::Sender,
) -> anyhow::Result<()> {
    /* Save struct */
    let mut buffer = questdb::ingress::Buffer::new();
    let table = buffer.table("binance_depth")?;

    for binance_update in updates {
        //tracing::debug!("binance={:?}", binance_update);
        let v: Value = serde_json::from_str(binance_update).unwrap();
        if v["BinanceDepthUpdate"].is_null() {
            tracing::debug!("❌ It doesn t have binance info");
            continue;
        }
        let inner = &v["BinanceDepthUpdate"];
        match serde_json::from_value::<BinanceDepthUpdate>(inner.clone()) {
            Ok(binance_update) => {
                table
                    .column_i64("u", binance_update.u as i64)?
                    .column_str("bid_json", serde_json::to_string(&binance_update.b)?)?
                    .column_str("ask_json", serde_json::to_string(&binance_update.a)?)?
                    .at(questdb::ingress::TimestampNanos::now())?;
            }
            Err(e) => {
                tracing::error!("❌ Error converting message to text: {}", e);
            }
        }
    }
    sender.flush(&mut buffer)?;
    Ok(())
}
