use std::time::Duration;

use tokio::sync::broadcast;
use tokio::time::sleep;

use super::Worker;

pub struct BitcoinWorker;
#[async_trait::async_trait(?Send)]
impl Worker for BitcoinWorker {
    async fn do_work(&mut self, id: usize, tx_cloned: broadcast::Sender<String>) {
        tracing::info!("Task {} starting...", id);
        for count in 0..100 {
            // Simulate some work
            tracing::info!("Task {} doing work...", id);
            sleep(Duration::from_millis(1000)).await;
            tx_cloned
                .send(format!("Task {} data! {}", id, count))
                .unwrap();
        }
        tracing::info!("Task {} done!", id);
    }
}
