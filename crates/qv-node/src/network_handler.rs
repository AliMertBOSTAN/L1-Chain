//! Network handler — bridges libp2p gossip events to the Node event loop.
//!
//! The `NetworkHandler` subscribes to `NetEvent`s from the `NetworkNode` and
//! decodes gossip messages into `NodeEvent`s that flow through the main loop.
//! It also provides convenience methods for publishing messages to the network.

use crate::node::NodeEvent;
use qv_core::{Block, Transaction};
use qv_net::{NetEvent, NetError, NetworkMessage, NetworkNode};
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Bridges network events to the node's event loop.
///
/// Holds a receiver channel from the `NetworkNode` and forwards decoded messages
/// as `NodeEvent`s to the node's event channel.
pub struct NetworkHandler {
    /// Receiver for network events from the libp2p layer.
    event_rx: mpsc::UnboundedReceiver<NetEvent>,
    /// Sender for node events (to the main loop).
    event_tx: mpsc::Sender<NodeEvent>,
}

impl NetworkHandler {
    /// Create a new network handler.
    ///
    /// # Parameters
    /// - `event_rx`: Receiver channel from `NetworkNode::take_event_receiver()`.
    /// - `event_tx`: Sender channel to the node's main event loop.
    ///
    /// # Returns
    /// A new `NetworkHandler` ready to be run.
    pub fn new(
        event_rx: mpsc::UnboundedReceiver<NetEvent>,
        event_tx: mpsc::Sender<NodeEvent>,
    ) -> Self {
        Self { event_rx, event_tx }
    }

    /// Run the event loop — process network events and forward them to the node.
    ///
    /// This is an async method that should be spawned on a tokio runtime. It
    /// continuously listens for `NetEvent`s from the network and:
    /// - Decodes gossip messages into the appropriate `NodeEvent` type
    /// - Forwards them to the node's event channel
    /// - Logs peer connection/disconnection events
    /// - Gracefully handles errors without panicking
    pub async fn run(mut self) {
        while let Some(event) = self.event_rx.recv().await {
            match event {
                NetEvent::Message { source, message } => {
                    self.handle_message(source, message).await;
                }
                NetEvent::PeerConnected(peer_id) => {
                    debug!(peer = %peer_id, "peer connected");
                    // Could emit metrics here; for now just log
                }
                NetEvent::PeerDisconnected(peer_id) => {
                    debug!(peer = %peer_id, "peer disconnected");
                    // Could emit metrics here; for now just log
                }
            }
        }
        debug!("network handler event loop ended");
    }

    /// Handle a decoded network message.
    ///
    /// Routes the message to the appropriate handler based on type.
    async fn handle_message(&self, _source: qv_net::PeerId, message: NetworkMessage) {
        let result = match message {
            NetworkMessage::Block(block) => {
                debug!("received block from network");
                self.event_tx
                    .send(NodeEvent::BlockReceived(*block))
                    .await
                    .map_err(|e| {
                        format!("failed to forward block to node event loop: {e}")
                    })
            }
            NetworkMessage::Transaction(tx) => {
                debug!("received transaction from network");
                self.event_tx
                    .send(NodeEvent::TxReceived(*tx))
                    .await
                    .map_err(|e| {
                        format!("failed to forward transaction to node event loop: {e}")
                    })
            }
            NetworkMessage::VrfProof(vrf) => {
                debug!(slot = vrf.slot, "received VRF proof from network (not yet handled)");
                Ok(())
            }
            NetworkMessage::Vote(vote) => {
                debug!(slot = vote.slot, "received vote from network (not yet handled)");
                Ok(())
            }
            // Request-response and ping/pong messages are not gossip-propagated
            // and should not appear here; log as unexpected.
            NetworkMessage::GetHeaders(_) => {
                warn!("unexpected GetHeaders message in gossip stream");
                Ok(())
            }
            NetworkMessage::Headers(_) => {
                warn!("unexpected Headers message in gossip stream");
                Ok(())
            }
            NetworkMessage::GetBlocks(_) => {
                warn!("unexpected GetBlocks message in gossip stream");
                Ok(())
            }
            NetworkMessage::Ping(_) => {
                warn!("unexpected Ping message in gossip stream");
                Ok(())
            }
            NetworkMessage::Pong(_) => {
                warn!("unexpected Pong message in gossip stream");
                Ok(())
            }
        };

        if let Err(e) = result {
            warn!("network message handling error: {}", e);
        }
    }

    /// Publish a block to the gossip network.
    ///
    /// This is a static helper method that can be called from the node or other
    /// components to broadcast a block via gossip.
    pub fn publish_block(network_node: &mut NetworkNode, block: &Block) -> Result<(), NetError> {
        let msg = NetworkMessage::Block(Box::new(block.clone()));
        network_node.publish(&msg)
    }

    /// Publish a transaction to the gossip network.
    ///
    /// This is a static helper method that can be called from the node or other
    /// components to broadcast a transaction via gossip.
    pub fn publish_transaction(
        network_node: &mut NetworkNode,
        tx: &Transaction,
    ) -> Result<(), NetError> {
        let msg = NetworkMessage::Transaction(Box::new(tx.clone()));
        network_node.publish(&msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qv_core::{
        Amount, Block, BlockHash, BlockHeader, Hash256, Height, MerkleRoot, OutPoint, Script,
        Slot, Timestamp, TxId, TxInput, TxOutput, UtxoCommitment, BLOCK_VERSION,
    };
    use qv_net::PeerId;

    /// Helper: a minimal transaction (one zero-input, one zero-value output).
    fn dummy_transaction() -> Transaction {
        let prev = OutPoint::new(TxId::from_bytes([0u8; 32]), 0);
        Transaction::new(
            vec![TxInput::new(prev)],
            vec![TxOutput::new(Amount::from_smallest_units(100), Script::new(vec![]))],
        )
    }

    /// Helper: a minimal block with no transactions (height 0).
    fn dummy_block() -> Block {
        let header = BlockHeader {
            version: BLOCK_VERSION,
            prev_hash: BlockHash::ZERO,
            height: Height::GENESIS,
            slot: Slot::GENESIS,
            timestamp: Timestamp::from_unix_secs(0),
            merkle_root: MerkleRoot::ZERO,
            utxo_commitment: UtxoCommitment::ZERO,
            vrf_proof: vec![],
            kes_sig: vec![],
            producer_key_hash: Hash256::ZERO,
        };
        Block::new(header, vec![])
    }

    #[tokio::test]
    async fn test_block_message_decoding() {
        let (event_tx, mut event_rx) = mpsc::channel(10);
        let (network_tx, network_rx) = mpsc::unbounded_channel();

        let handler = NetworkHandler::new(network_rx, event_tx);

        // Spawn the handler
        let handler_task = tokio::spawn(async move {
            handler.run().await;
        });

        // Send a block message via the network event channel
        let block = dummy_block();
        let net_event = NetEvent::Message {
            source: PeerId::random(),
            message: NetworkMessage::Block(Box::new(block.clone())),
        };
        network_tx.send(net_event).unwrap();

        // Close the sender so the handler task exits
        drop(network_tx);

        // Receive and verify the forwarded event
        let received = event_rx.recv().await.expect("should receive block event");
        match received {
            NodeEvent::BlockReceived(b) => {
                assert_eq!(b.header.height, block.header.height);
            }
            _ => panic!("expected BlockReceived event"),
        }

        // Wait for handler task to complete
        handler_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_transaction_message_decoding() {
        let (event_tx, mut event_rx) = mpsc::channel(10);
        let (network_tx, network_rx) = mpsc::unbounded_channel();

        let handler = NetworkHandler::new(network_rx, event_tx);

        let handler_task = tokio::spawn(async move {
            handler.run().await;
        });

        let tx = dummy_transaction();
        let net_event = NetEvent::Message {
            source: PeerId::random(),
            message: NetworkMessage::Transaction(Box::new(tx.clone())),
        };
        network_tx.send(net_event).unwrap();
        drop(network_tx);

        let received = event_rx.recv().await.expect("should receive tx event");
        match received {
            NodeEvent::TxReceived(t) => {
                assert_eq!(t.inputs.len(), tx.inputs.len());
                assert_eq!(t.outputs.len(), tx.outputs.len());
            }
            _ => panic!("expected TxReceived event"),
        }

        handler_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_unknown_message_types_logged() {
        let (event_tx, _event_rx) = mpsc::channel(10);
        let (network_tx, network_rx) = mpsc::unbounded_channel();

        let handler = NetworkHandler::new(network_rx, event_tx);

        let handler_task = tokio::spawn(async move {
            handler.run().await;
        });

        // Send a Ping message (which should not appear in gossip)
        let net_event = NetEvent::Message {
            source: PeerId::random(),
            message: NetworkMessage::Ping(qv_net::message::PingMsg { nonce: 42 }),
        };
        network_tx.send(net_event).unwrap();
        drop(network_tx);

        // Handler should process it without panicking
        handler_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_peer_connected_event() {
        let (event_tx, _event_rx) = mpsc::channel(10);
        let (network_tx, network_rx) = mpsc::unbounded_channel();

        let handler = NetworkHandler::new(network_rx, event_tx);

        let handler_task = tokio::spawn(async move {
            handler.run().await;
        });

        let peer_id = PeerId::random();
        let net_event = NetEvent::PeerConnected(peer_id);
        network_tx.send(net_event).unwrap();
        drop(network_tx);

        // Handler should process the peer connected event gracefully
        handler_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_peer_disconnected_event() {
        let (event_tx, _event_rx) = mpsc::channel(10);
        let (network_tx, network_rx) = mpsc::unbounded_channel();

        let handler = NetworkHandler::new(network_rx, event_tx);

        let handler_task = tokio::spawn(async move {
            handler.run().await;
        });

        let peer_id = PeerId::random();
        let net_event = NetEvent::PeerDisconnected(peer_id);
        network_tx.send(net_event).unwrap();
        drop(network_tx);

        // Handler should process the peer disconnected event gracefully
        handler_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_channel_closure_exits_loop() {
        let (event_tx, _event_rx) = mpsc::channel(10);
        let (network_tx, network_rx) = mpsc::unbounded_channel();

        let handler = NetworkHandler::new(network_rx, event_tx);

        let handler_task = tokio::spawn(async move {
            handler.run().await;
        });

        // Drop the sender NOW so the handler's `recv()` returns None and
        // its loop exits. (`let (_network_tx, ...) = ...` would have kept
        // the sender alive until end-of-scope and hung the await forever.)
        drop(network_tx);
        handler_task.await.unwrap();
    }
}
