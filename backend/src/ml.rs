use serde_json::Value;
use tokio::sync::broadcast::Sender;
use zmq::Message;

fn send_zmq(endpoint: &str, data: String) -> anyhow::Result<String> {
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::REQ).unwrap();
    socket.connect(endpoint).unwrap();
    socket.send(data.as_str(), 0).unwrap();
    let reply = socket.recv_string(0)?.unwrap(); // Required in REQ
    println!("Got reply: {:?}", reply);
    Ok(reply)
}

pub async fn run_ml_inter(
    updates: &Vec<String>,
    tx: Sender<String>,
    endpoint: String,
) -> anyhow::Result<()> {
    for binance_update in updates {
        //tracing::debug!("binance={:?}", binance_update);
        let v: Value = serde_json::from_str(binance_update).unwrap();
        if v["BinanceDepthUpdate"].is_null() {
            tracing::debug!("❌ It doesn t have binance info");
            continue;
        }
        let rest = send_zmq(endpoint.as_str(), binance_update.clone()).unwrap();
        tx.send(rest.clone())?; // Send the candle to the channel
    }
    tracing::debug!("Running inference for ml solution!");
    Ok(())
}
