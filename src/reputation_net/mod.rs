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

use super::model::{today, Entity, EntityType, Opinion, SignedStatement, Statement};
use super::storage::{Id, Storage};

#[derive(Debug)]
pub enum Event {
    NewStatement(SignedStatement, Option<PeerId>),
    TemplateRequest(Option<PeerId>),
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
        /* for now, create opinions with default values. I don't know yet how the UI should look finally */

        match what.parse() {
            Ok(statement) => match self
                .storage
                .persist_statement_hashing_emails(&statement)
                .await
            {
                Ok((persist_result, actual_statement)) => {
                    println!(
                        "{} statement {} has id {}",
                        persist_result.wording(),
                        actual_statement,
                        persist_result.id
                    );
                    let signed_statement = self
                        .sign_statement(actual_statement, persist_result.id)
                        .await
                        .unwrap();
                    // we ignore the possible error when no peers are currently connected
                    self.publish_statement(&signed_statement).await;
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

    pub async fn sign_statement(
        &mut self,
        statement: Statement,
        statement_id: Id,
    ) -> Option<SignedStatement> {
        let opinion = Opinion {
            date: today(),
            valid: 30,
            serial: 0,
            certainty: 3,
            comment: "".into(),
        };
        let trust = self.storage.owner_trust().await.unwrap();
        if let Some(keypair) = trust.key {
            let signed_opinion = opinion.sign_using(&statement.signable_bytes(), &keypair);
            self.storage
                .persist_opinion(&signed_opinion, statement_id)
                .await
                .unwrap();
            Some(SignedStatement {
                statement: statement,
                opinions: vec![signed_opinion],
            })
        } else {
            None
        }
    }

    pub async fn publish_statement(&mut self, signed_statement: &SignedStatement) {
        match self.gossipsub.publish(
            self.as_topic(&signed_statement.statement.name),
            signed_statement.to_string(),
        ) {
            Ok(_mid) => info!("published ok"),
            Err(err) => error!("could not publish: {:?}", err),
        };
    }

    pub async fn handle_event(&mut self, event: &Event) {
        debug!("got event: {:?}", event);
        match event {
            Event::NewStatement(signed_statement, _peer) => {
                let statement = &signed_statement.statement;
                match self.storage.persist_statement(statement).await {
                    Ok(persist_result) => {
                        info!(
                            "{} statement {} has id {}",
                            persist_result.wording(),
                            statement,
                            persist_result.id
                        );
                        for signed_opinion in &signed_statement.opinions {
                            let result = self
                                .storage
                                .persist_opinion(signed_opinion, persist_result.id)
                                .await
                                .expect("could insert opinion");
                            info!(
                                "{} opinion {} has id {}",
                                result.wording(),
                                signed_opinion,
                                result.id
                            );
                        }
                        if persist_result.is_new() && statement.name == "template" {
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
                let entities = self
                    .storage
                    .list_entities(EntityType::Template)
                    .await
                    .unwrap();
                for entity in entities {
                    let statement = Statement {
                        name: "template".into(),
                        entities: vec![entity],
                    };
                    let opinion = Opinion::default();
                    if let Ok(trust) = self.storage.owner_trust().await {
                        if let Some(keypair) = trust.key {
                            let signed_statement = SignedStatement {
                                opinions: vec![
                                    opinion.sign_using(&statement.signable_bytes(), &keypair)
                                ],
                                statement: statement,
                            };
                            self.publish_statement(&signed_statement).await;
                        }
                    }
                }
            }
        }
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for ReputationNet {
    // Called when `Gossipsub` produces an event.
    fn inject_event(&mut self, event: GossipsubEvent) {
        debug!("Gossipsub event: {:?}", event);
        match event {
            GossipsubEvent::Message {
                propagation_source,
                message_id,
                message,
            } => {
                let string = String::from_utf8_lossy(&message.data);
                match string.parse::<SignedStatement>() {
                    Ok(signed_statement) => {
                        debug!(
                            "Received: {} from {:?}, id {:?}",
                            signed_statement, propagation_source, message_id
                        );
                        match self.event_sender.try_send(Event::NewStatement(
                            signed_statement,
                            message.source.clone(),
                        )) {
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
                    debug!("peer {} wants topics, using broadcast for now", peer_id);
                    self.event_sender
                        .try_send(Event::TemplateRequest(Some(peer_id)))
                        .unwrap();
                }
            }
            _ => (),
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for ReputationNet {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        debug!("mdns event: {:?}", event);
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
    fn inject_event(&mut self, event: PingEvent) {
        debug!("ping event: {:?}", event);
    }
}