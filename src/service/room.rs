use std::collections::HashMap;

use coin_shuffle_core::service::types::EncodedOuput;
use coin_shuffle_protos::v1::ShuffleEvent;
use ethers_core::types::{Signature, U256};
use tokio::sync::mpsc::{Receiver as StreamReceiver, Sender as StreamSender};

pub enum RoomEvents {
    ShuffleRound((U256, Vec<EncodedOuput>)),
    SignedOutput((U256, Signature)),
}

pub struct RoomConnection {
    events: StreamReceiver<RoomEvents>,
    participant_streams: HashMap<U256, StreamSender<Result<ShuffleEvent, tonic::Status>>>,
}

impl RoomConnection {
    pub fn new(events: StreamReceiver<RoomEvents>) -> Self {
        Self {
            events,
            participant_streams: HashMap::new(),
        }
    }

    pub fn add_participant(
        &mut self,
        id: U256,
        stream: StreamSender<Result<ShuffleEvent, tonic::Status>>,
    ) {
        self.participant_streams.insert(id, stream);
    }

    pub async fn run(&mut self) {
        // while let Some(event) = self.events.recv().await {
        //     for (_, stream) in self.participant_streams.iter_mut() {
        //         stream.send(Ok(event.clone())).await.unwrap();
        //     }
        // }
    }
}
