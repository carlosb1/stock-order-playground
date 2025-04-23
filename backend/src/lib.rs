use tokio::sync::broadcast;

pub mod binance;
pub mod bitcoin;
pub mod coinbase;
pub mod mock;

mod models;

mod db;
mod ml;

//#[async_trait::async_trait(?Send)]
#[async_trait::async_trait(?Send)]
pub trait Worker: Send + Sync + 'static {
    async fn do_work(&mut self, id: usize, tx: broadcast::Sender<String>);
}
