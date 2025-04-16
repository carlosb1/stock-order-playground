use tokio::sync::mpsc::Sender;

pub mod bitcoin;
pub mod mock;
pub mod coinbase;
pub mod binance;

mod models;

mod db;
//#[async_trait::async_trait(?Send)]
#[async_trait::async_trait(?Send)]
pub trait Worker: Send + Sync + 'static {
    async fn do_work(&mut self, id: usize, tx: Sender<String>);
}
