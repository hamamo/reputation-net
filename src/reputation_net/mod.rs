use async_std::task::block_on;

#[allow(unused_imports)]
use libp2p::{
    core::connection::{ConnectedPoint, ConnectionId},
    floodsub::{self, Floodsub, FloodsubEvent, Topic},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    ping::{Ping, PingConfig, PingEvent},
    swarm::{
        IntoProtocolsHandler, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
        ProtocolsHandler, SwarmEvent,
    },
    NetworkBehaviour, PeerId,
};

use super::storage::Storage;

#[derive(NetworkBehaviour)]
pub struct ReputationNet {
    #[cfg(floodsub)]
    floodsub: Floodsub,
    #[cfg(mdns)]
    mdns: Mdns,
    ping: Ping,

    #[behaviour(ignore)]
    #[cfg(floodsub)]
    topic: Topic,
    #[behaviour(ignore)]
    storage: Storage,
}

impl ReputationNet {
    #[allow(unused_variables)]
    pub async fn new(local_peer_id: PeerId) -> Self {
        #[allow(unused_mut)]
        let mut result = Self {
            #[cfg(floodsub)]
            floodsub: Floodsub::new(local_peer_id),
            #[cfg(mdns)]
            mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
            ping: Ping::new(PingConfig::new().with_keep_alive(true)),
            #[cfg(floodsub)]
            topic: Topic::new("greeting"),
            storage: Storage::new().await,
        };
        #[cfg(floodsub)]
        result.floodsub.subscribe(result.topic.clone());
        result
    }

    pub fn handle_input(&mut self, what: &str) {
        /* for now, interpret input lines as entities and store them */
        match what.parse() {
            Ok(statement) => match block_on(self.storage.lookup_statement(&statement)) {
                Ok((id, inserted)) => println!(
                    "{} statement {} has id {}",
                    if inserted {
                        "newly inserted"
                    } else {
                        "previously existing"
                    },
                    &statement,
                    &id
                ),
                e => println!("No matching template: {:?}", e),
            },
            e => println!("Invalid statement format: {:?}", e),
        };
    }
}

#[cfg(floodsub)]
impl NetworkBehaviourEventProcess<FloodsubEvent> for ReputationNet {
    // Called when `floodsub` produces an event.
    fn inject_event(&mut self, message: FloodsubEvent) {
        if let FloodsubEvent::Message(message) = message {
            println!(
                "Received: '{:?}' from {:?}",
                String::from_utf8_lossy(&message.data),
                message.source
            );
        }
    }
}

#[cfg(mdns)]
impl NetworkBehaviourEventProcess<MdnsEvent> for ReputationNet {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, addr) in list {
                    println!("discovered {} {}", peer, addr);
                    #[cfg(floodsub)]
                    self.floodsub.add_node_to_partial_view(peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, addr) in list {
                    println!("expired {} {}", peer, addr);
                    if !self.mdns.has_node(&peer) {
                        #[cfg(floodsub)]
                        self.floodsub.remove_node_from_partial_view(&peer);
                    }
                }
            }
        }
    }
}

// implemented to keep connections alive
impl NetworkBehaviourEventProcess<PingEvent> for ReputationNet {
    #[allow(unused_variables)]
    fn inject_event(&mut self, event: PingEvent) {
        // println!("{:?}", event);
    }
}
