
use futures::{channel::mpsc::Sender};
use log::{debug, error, info};

#[allow(unused_imports)]
use libp2p::{
    core::connection::{ConnectedPoint, ConnectionId},
    gossipsub::{
        self, Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity,
        MessageId,
    },
    identity::Keypair,
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    swarm::{
        IntoProtocolsHandler, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
        ProtocolsHandler, SwarmEvent,
    },
    NetworkBehaviour, PeerId,
};



use super::model::Statement;
use super::storage::Storage;

#[derive(Debug)]
pub enum Event {
    NewStatement(Statement, Option<PeerId>),
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
    event_sender: Sender<Event>,
}

impl ReputationNet {
    #[allow(unused_variables)]
    pub async fn new(local_peer_id: PeerId, event_sender: Sender<Event>) -> Self {
        let keypair = Keypair::generate_ed25519();
        #[allow(unused_mut)]
        let mut result = Self {
            gossipsub: Gossipsub::new(
                MessageAuthenticity::Signed(keypair),
                GossipsubConfig::default(),
            )
            .unwrap(),
            mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
            topics: vec![IdentTopic::new("greeting")],
            storage: Storage::new().await,
            event_sender: event_sender,
        };
        for t in &result.topics {
            result.gossipsub.subscribe(t).expect("subscribe works");
        }
        result
    }

    pub async fn handle_input(&mut self, what: &str) {
        /* for now, interpret input lines as entities and store them */

        match what.parse() {
            Ok(statement) => match self.storage.lookup_statement_hashing_emails(&statement).await {
                Ok(((id, inserted), actual_statement)) => {
                    println!(
                        "{} statement {} has id {}",
                        if inserted {
                            "newly created"
                        } else {
                            "previously existing"
                        },
                        &actual_statement,
                        &id
                    );
                    // we ignore the possible error when no peers are currently connected
                    match self
                        .gossipsub
                        .publish(self.topics[0].clone(), actual_statement.to_string())
                    {
                        Ok(_mid) => println!("published ok"),
                        Err(err) => println!("could not publish: {:?}", err),
                    };
                }
                Err(_e) => {
                    println!("No matching template: {}", statement.minimal_template());
                    println!("Available:");
                    for t in self.storage.list_templates(&statement.name).await.iter() {
                        println!("  {}", t)
                    }
                },
            },
            Err(e) => println!("Invalid statement format: {:?}", e),
        };
    }

    pub async fn handle_event(&mut self, event: &Event) {
        debug!("got event: {:?}", event);
        match event {
            Event::NewStatement(statement, _peer) => {
                match self.storage.lookup_statement(&statement).await {
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
                    }
                    Err(e) => println!("No matching template for {}: {:?}", statement, e),
                }
            }
        }
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for ReputationNet {
    // Called when `Gossipsub` produces an event.
    fn inject_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message {
                propagation_source,
                message_id,
                message,
            } => {
                let string = String::from_utf8_lossy(&message.data);
                match string.parse::<Statement>() {
                    Ok(statement) => {
                        info!("Received: {} from {:?}, id {:?}", statement, propagation_source, message_id);
                        match self
                            .event_sender
                            .try_send(Event::NewStatement(statement, message.source.clone()))
                        {
                            Err(e) => error!("could not send event: {:?}", e),
                            _ => (),
                        }
                    }
                    Err(e) => {
                        error!("could not parse {:?}: {:?}", string, e);
                    }
                }
            }
            _ => info!("Gossipsub event: {:?}", event),
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for ReputationNet {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, addr) in list {
                    info!("discovered {} {}", peer, addr);
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
