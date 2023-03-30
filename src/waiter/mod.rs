///! This mode provides a subservice for managing participants that havn't yet
///! connected to their rooms to start shuffle.
///!
///! The main abstraction here is [`Waiter`] that manages queues of participants
///! waiting for a room to be ready and signals when a queue is filled.
///!
///! The queue is represented by uniqe keys of the room: ERC20 token address and
///! amount that will be shuffled. Participant in that room represented by his UTXO
///! identifier.
mod queue;

use ethers_core::types::{Address, U256};

#[derive(Clone)]
pub struct Waiter {
    ///! The queue of participants waiting for a room to be ready.
    queue: queue::QueuesStorage,
    ///! Number of participants that should be in a room to start shuffle.
    min_participants: usize,
}

impl Waiter {
    pub fn new(min_participants: usize) -> Self {
        Self {
            queue: queue::QueuesStorage::new(),
            min_participants,
        }
    }

    /// Adds a participant to the queue. Returns participants if the queue is filled.
    pub async fn add_participant(
        &self,
        token: Address,
        amount: U256,
        participant: U256,
    ) -> Option<Vec<U256>> {
        self.queue.push(token, amount, participant).await;

        if self.is_filled(token, amount).await {
            Some(self.queue.pop(token, amount).await)
        } else {
            None
        }
    }

    async fn is_filled(&self, token: Address, amount: U256) -> bool {
        self.queue.len(token, amount).await >= self.min_participants
    }
}
