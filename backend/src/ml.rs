use extism::{Manifest, Plugin, Wasm};
use serde_json::Value;
use tokio::sync::broadcast::Sender;
use zmq::Message;

fn send_zmq(endpoint: &str, data: String) -> anyhow::Result<()> {
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::REQ).unwrap();
    socket.connect(endpoint).unwrap();
    socket.send(data.as_str(), 0).unwrap();
    let reply = socket.recv_string(0)?; // Required in REQ
    println!("Got reply: {:?}", reply);
    Ok(())
}

pub async fn run_ml_inter(updates: &Vec<String>, tx: Sender<String>) -> anyhow::Result<()> {
    for binance_update in updates {
        //tracing::debug!("binance={:?}", binance_update);
        let v: Value = serde_json::from_str(binance_update).unwrap();
        if v["BinanceDepthUpdate"].is_null() {
            tracing::debug!("❌ It doesn t have binance info");
            continue;
        }
        send_zmq("tcp://0.0.0.0:5555", binance_update.clone()).unwrap();
    }
    tracing::debug!("Running inference for ml solution!");
    /* Save struct */
    /*
    let url =
        Wasm::url("https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm");
    let manifest = Manifest::new([url]);
    let mut plugin = Plugin::new(&manifest, [], true).unwrap();
    let res = plugin
        .call::<&str, &str>("count_vowels", "Hello, world!")
        .unwrap();
    tracing::debug!("{}", res);
    tx.send(res.to_string()).unwrap();
    */
    Ok(())
}
