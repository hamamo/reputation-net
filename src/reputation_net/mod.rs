use async_std::task::{block_on, Poll};
use futures::{prelude::*};

use libp2p::{gossipsub::{GossipsubConfig, GossipsubEvent, MessageAuthenticity}, identity::Keypair};
#[allow(unused_imports)]
use libp2p::{
    core::connection::{ConnectedPoint, ConnectionId},
    gossipsub::{self, Gossipsub, IdentTopic},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    swarm::{
        IntoProtocolsHandler, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
        ProtocolsHandler, SwarmEvent,
    },
    NetworkBehaviour, PeerId,
};

use super::model::Statement;
use super::storage::Storage;

enum Event {
    NewStatement(Statement, PeerId),
}

#[derive(NetworkBehaviour)]
pub struct ReputationNet {
    mdns: Mdns,
    gossipsub: Gossipsub,

    #[behaviour(ignore)]
    topics: Vec<IdentTopic>,
    #[behaviour(ignore)]
    storage: Storage,
    #[behaviour(ignore)]
    events: Vec<Event>,
}

impl ReputationNet {
    #[allow(unused_variables)]
    pub async fn new(local_peer_id: PeerId) -> Self {
        let keypair = Keypair::generate_ed25519();
        #[allow(unused_mut)]
        let mut result = Self {
            gossipsub: Gossipsub::new(MessageAuthenticity::Signed(keypair), GossipsubConfig::default()).unwrap(),
            mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
            topics: vec![IdentTopic::new("greeting")],
            storage: Storage::new().await,
            events: vec![],
        };
        for t in &result.topics {
            result.gossipsub.subscribe(t);
        }
        result
    }

    pub fn handle_input(&mut self, what: &str) {
        /* for now, interpret input lines as entities and store them */
        match what.parse() {
            Ok(statement) => match block_on(self.storage.lookup_statement(&statement)) {
                Ok((id, inserted)) => {
                    println!(
                        "{} statement {} has id {}",
                        if inserted {
                            "newly created"
                        } else {
                            "previously existing"
                        },
                        &statement,
                        &id
                    );
                    self.gossipsub
                        .publish(self.topics[0], statement.to_string());
                }
                e => println!("No matching template: {:?}", e),
            },
            e => println!("Invalid statement format: {:?}", e),
        };
    }

    pub fn handle_events(&mut self) {
        while let Some(event) = self.events.pop() {
            match event {
                Event::NewStatement(stmt, peer_id) => {
                    println!("new statement {} from peer {}", stmt, peer_id);
                }
                _ => ()
            }
        }
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for ReputationNet {
    // Called when `Gossipsub` produces an event.
    fn inject_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message{propagation_source, message_id, message} => {
                if let Ok(statement) = (String::from_utf8_lossy(&message.data)).parse::<Statement>() {
                    println!("Received: {} from {:?}", statement, message);
                } 
            },
            _ => println!("Gossipsub event: {:?}", event)
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for ReputationNet {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, addr) in list {
                    println!("discovered {} {}", peer, addr);
                    self.gossipsub.add_explicit_peer(&peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, addr) in list {
                    println!("expired {} {}", peer, addr);
                    if !self.mdns.has_node(&peer) {
                        self.gossipsub.remove_explicit_peer(&peer);
                    }
                }
            }
        }
    }
}