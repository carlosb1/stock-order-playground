use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::time::sleep;
use super::Worker;

#[derive(Clone)]
pub struct MockWorker;
#[async_trait::async_trait(?Send)]
impl Worker for MockWorker {
    async fn do_work(&mut self, id: usize, tx_cloned: Sender<String>) {
        println!("Task {} starting...", id);
        for count in 0..100 {
            // Simulate some work
            println!("Task {} doing work...", id);
            sleep(Duration::from_millis(1000)).await;
            tx_cloned.send(format!("Task {} data! {}", id, count)).await.unwrap();
        }
        println!("Task {} done!", id);
    }
}