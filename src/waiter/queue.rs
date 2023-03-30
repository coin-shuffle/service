use std::{collections::HashMap, sync::Arc};

use ethers_core::types::{Address, U256};
use tokio::sync::Mutex;

/// Storage of vectors of participants, where participants is represented by his UTXO id
/// and key of the queue is a pair of (token address, amount).
#[derive(Clone)]
pub struct QueuesStorage {
    queues: Arc<Mutex<HashMap<(Address, U256), Vec<U256>>>>,
}

impl QueuesStorage {
    pub fn new() -> Self {
        Self {
            queues: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn push(&self, token: Address, amount: U256, utxo_id: U256) {
        let mut queues = self.queues.lock().await;
        let queue = queues.entry((token, amount)).or_insert_with(Vec::new);
        queue.push(utxo_id);
    }

    /// Clean up the queue and return the list of participants.
    pub async fn pop(&self, token: Address, amount: U256) -> Vec<U256> {
        let mut queues = self.queues.lock().await;
        let queue = queues.entry((token, amount)).or_insert_with(Vec::new);
        let participants = queue.clone();
        queue.clear();
        participants
    }

    pub async fn len(&self, token: Address, amount: U256) -> usize {
        let queues = self.queues.lock().await;

        let Some(queue) = queues.get(&(token, amount)) else {
            return 0;
        };

        queue.len()
    }
}
