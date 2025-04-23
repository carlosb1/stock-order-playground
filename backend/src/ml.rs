use extism::{Manifest, Plugin, Wasm};
use serde_json::Value;
use tokio::sync::broadcast::Sender;

pub async fn run_ml_inter(updates: &Vec<String>, tx: Sender<String>) -> anyhow::Result<()> {
    /* Save struct */
    let url =
        Wasm::url("https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm");
    let manifest = Manifest::new([url]);
    let mut plugin = Plugin::new(&manifest, [], true).unwrap();

    for binance_update in updates {
        //tracing::debug!("binance={:?}", binance_update);
        let v: Value = serde_json::from_str(binance_update).unwrap();
        if v["BinanceDepthUpdate"].is_null() {
            tracing::debug!("❌ It doesn t have binance info");
            continue;
        }
    }
    tracing::debug!("Running inference for ml solution!");
    let res = plugin
        .call::<&str, &str>("count_vowels", "Hello, world!")
        .unwrap();
    tracing::debug!("{}", res);
    tx.send(res.to_string()).unwrap();
    Ok(())
}
