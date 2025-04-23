use chrono::Utc;
use questdb::{
    Result,
    ingress::{Buffer, Sender, TimestampNanos},
};

fn main() -> Result<()> {
    let mut sender = Sender::from_conf(
        "http::addr=localhost:9000;username=admin;password=quest;retry_timeout=20000;",
    )?;
    let mut buffer = Buffer::new();
    let current_datetime = Utc::now();

    buffer
        .table("trades")?
        .symbol("symbol", "ETH-USD")?
        .symbol("side", "sell")?
        .column_f64("price", 2615.54)?
        .column_f64("amount", 0.00044)?
        .at(TimestampNanos::from_datetime(current_datetime)?)?;

    sender.flush(&mut buffer)?;
    Ok(())
}
