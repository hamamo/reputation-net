use std::collections::HashSet;

use futures::channel::mpsc::Sender;

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
    ping::{Ping, PingConfig, PingEvent},
    swarm::{
        IntoProtocolsHandler, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
        ProtocolsHandler, SwarmEvent,
    },
    NetworkBehaviour, PeerId,
};

use crate::model::{Entity, EntityType};

use super::model::Statement;
use super::storage::Storage;

#[derive(Debug)]
pub enum Event {
    NewStatement(Statement, Option<PeerId>),
    TemplateRequest(Option<PeerId>)
}

#[derive(NetworkBehaviour)]
pub struct ReputationNet {
    mdns: Mdns,
    gossipsub: Gossipsub,
    ping: Ping,

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
        let mut repnet = Self {
            gossipsub: Gossipsub::new(
                MessageAuthenticity::Signed(keypair),
                GossipsubConfig::default(),
            )
            .unwrap(),
            mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
            ping: Ping::new(PingConfig::new()),
            storage: Storage::new().await,
            event_sender: event_sender,
        };
        for t in &repnet.topics().await {
            repnet
                .gossipsub
                .subscribe(&IdentTopic::new(t))
                .expect("subscribe works");
        }
        repnet
    }

    fn as_topic(&self, s: &str) -> IdentTopic {
        IdentTopic::new(s)
    }

    pub async fn topics(&self) -> Vec<String> {
        match self.storage.list_entities(EntityType::Template).await {
            Ok(templates) => templates
                .into_iter()
                .map(|entity| match entity {
                    Entity::Template(template) => template.name.to_string(),
                    _ => "".into(),
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect(),
            Err(_) => vec![],
        }
    }

    pub async fn handle_input(&mut self, what: &str) {
        /* for now, interpret input lines as entities and store them */

        match what.parse() {
            Ok(statement) => match self
                .storage
                .lookup_statement_hashing_emails(&statement)
                .await
            {
                Ok(((id, inserted), actual_statement)) => {
                    println!(
                        "{} statement {} has id {}",
                        if inserted {
                            "newly created"
                        } else {
                            "previously existing"
                        },
                        actual_statement,
                        id
                    );
                    // we ignore the possible error when no peers are currently connected
                    self.publish_statement(&actual_statement).await;
                }
                Err(_e) => {
                    println!("No matching template: {}", statement.minimal_template());
                    println!("Available:");
                    for t in self.storage.list_templates(&statement.name).await.iter() {
                        println!("  {}", t)
                    }
                }
            },
            Err(e) => println!("Invalid statement format: {:?}", e),
        };
    }

    pub async fn publish_statement(&mut self, statement: &Statement) {
        match self
            .gossipsub
            .publish(self.as_topic(&statement.name), statement.to_string())
        {
            Ok(_mid) => info!("published ok"),
            Err(err) => error!("could not publish: {:?}", err),
        };
    }

    pub async fn handle_event(&mut self, event: &Event) {
        debug!("got event: {:?}", event);
        match event {
            Event::NewStatement(statement, _peer) => {
                match self.storage.lookup_statement(&statement).await {
                    Ok((id, inserted)) => {
                        info!(
                            "{} statement {} has id {}",
                            if inserted {
                                "newly created"
                            } else {
                                "previously existing"
                            },
                            &statement,
                            &id
                        );
                        if inserted && statement.name == "template" {
                            if let Entity::Template(template) = &statement.entities[0] {
                                self.gossipsub
                                    .subscribe(&self.as_topic(&template.name))
                                    .unwrap();
                            };
                        }
                    }
                    Err(e) => error!("No matching template for {}: {:?}", statement, e),
                }
            }
            Event::TemplateRequest(_peer) => {
                let entities = self.storage.list_entities(EntityType::Template).await.unwrap();
                for entity in entities {
                    self.publish_statement(&Statement {
                        name: "template".into(),
                        entities: vec![entity],
                    }).await
                };
            }
        }
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for ReputationNet {
    // Called when `Gossipsub` produces an event.
    fn inject_event(&mut self, event: GossipsubEvent) {
        info!("Gossipsub event: {:?}", event);
        match event {
            GossipsubEvent::Message {
                propagation_source,
                message_id,
                message,
            } => {
                let string = String::from_utf8_lossy(&message.data);
                match string.parse::<Statement>() {
                    Ok(statement) => {
                        info!(
                            "Received: {} from {:?}, id {:?}",
                            statement, propagation_source, message_id
                        );
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
            GossipsubEvent::Subscribed { peer_id, topic } => {
                if topic.as_str() == "template" {
                    info!("peer {} wants topics, using broadcast for now", peer_id);
                    self.event_sender.try_send(Event::TemplateRequest(Some(peer_id))).unwrap();
                }
            }
            _ => (),
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for ReputationNet {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        info!("mdns event: {:?}", event);
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, _addr) in list {
                    self.gossipsub.add_explicit_peer(&peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _addr) in list {
                    if !self.mdns.has_node(&peer) {
                        self.gossipsub.remove_explicit_peer(&peer);
                    }
                }
            }
        }
    }
}

impl NetworkBehaviourEventProcess<PingEvent> for ReputationNet {
    fn inject_event(&mut self, _event: PingEvent) {
        // info!("ping event: {:?}", event);
    }
}
