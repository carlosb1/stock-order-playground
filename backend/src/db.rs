use chrono::{DateTime, Utc};
use crate::models::{Candle, Level2, Side};

pub fn save_questdb_candle(candle: Candle, db_config: &String) -> anyhow::Result<()> {
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

pub fn save_questdb_event(level2_event: Level2, db_config: &String) -> anyhow::Result<()> {
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
