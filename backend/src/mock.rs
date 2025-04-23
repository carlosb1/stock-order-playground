use super::Worker;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;
use tokio::time::sleep;

#[derive(Clone)]
pub struct MockWorker;
#[async_trait::async_trait(?Send)]
impl Worker for MockWorker {
    async fn do_work(&mut self, id: usize, tx_cloned: broadcast::Sender<String>) {
        tracing::debug!("Task {} starting...", id);
        loop {
            // Simulate some work
            tracing::debug!("Task {} doing work...", id);
            sleep(Duration::from_millis(30000)).await;
            tx_cloned.send("{I am live!}".to_string()).unwrap();
        }
    }
}
