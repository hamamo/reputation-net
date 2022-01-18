use std::{collections::HashSet, sync::Arc, time::Duration};

use async_std::sync::RwLock;

use futures::channel::mpsc::Sender;
use libp2p::request_response::{ProtocolSupport, RequestResponseMessage};
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
    request_response::{
        RequestResponse, RequestResponseCodec, RequestResponseConfig, RequestResponseEvent,
    },
    swarm::{
        IntoProtocolsHandler, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
        ProtocolsHandler, SwarmEvent,
    },
    NetworkBehaviour, PeerId,
};

use crate::{
    model::Date,
    storage::{PersistResult, Repository},
};

use super::model::{Entity, UnsignedOpinion, SignedStatement, Statement};
use super::storage::Storage;

mod messages;
mod rpc;
mod sync;
mod user_input;
pub use messages::*;
use rpc::*;
use sync::*;

pub type NetworkMessageWithPeerId = (NetworkMessage, PeerId);

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent")]
pub struct ReputationNet {
    mdns: Mdns,
    gossipsub: Gossipsub,
    ping: Ping,
    rpc: RequestResponse<RpcCodec>,

    #[behaviour(ignore)]
    pub storage: Arc<RwLock<Storage>>,
    #[behaviour(ignore)]
    event_sender: Sender<(NetworkMessage, PeerId)>,
    #[behaviour(ignore)]
    pub local_key: Keypair,
    #[behaviour(ignore)]
    sync_state: SyncState,
}

#[derive(Debug)]
pub enum OutEvent {
    Mdns(MdnsEvent),
    Gossipsub(GossipsubEvent),
    Ping(PingEvent),
    Rpc(RequestResponseEvent<NetworkMessage, NetworkMessage>),
}

impl From<MdnsEvent> for OutEvent {
    fn from(v: MdnsEvent) -> Self {
        Self::Mdns(v)
    }
}

impl From<GossipsubEvent> for OutEvent {
    fn from(v: GossipsubEvent) -> Self {
        Self::Gossipsub(v)
    }
}

impl From<PingEvent> for OutEvent {
    fn from(v: PingEvent) -> Self {
        Self::Ping(v)
    }
}

impl From<RequestResponseEvent<NetworkMessage, NetworkMessage>> for OutEvent {
    fn from(v: RequestResponseEvent<NetworkMessage, NetworkMessage>) -> Self {
        Self::Rpc(v)
    }
}

impl ReputationNet {
    #[allow(unused_variables)]
    pub async fn new(event_sender: Sender<NetworkMessageWithPeerId>) -> Self {
        let storage = Storage::new().await;
        let keypair = storage.own_key().key.clone();
        let storage = Arc::new(RwLock::new(storage));
        let local_peer_id = PeerId::from(keypair.public());
        #[allow(unused_mut)]
        let mut repnet = Self {
            gossipsub: Gossipsub::new(
                MessageAuthenticity::Signed(keypair.clone()),
                GossipsubConfig::default(),
            )
            .unwrap(),
            mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
            ping: Ping::new(
                PingConfig::new()
                    .with_interval(Duration::new(90, 0))
                    .with_keep_alive(true),
            ),
            rpc: RequestResponse::new(
                RpcCodec {},
                vec![(RpcProtocol::Version1, ProtocolSupport::Full)].into_iter(),
                RequestResponseConfig::default(),
            ),
            storage: storage.clone(),
            event_sender: event_sender,
            local_key: keypair.clone(),
            sync_state: SyncState::new(storage).await,
        };
        for t in repnet.topics().await {
            repnet
                .gossipsub
                .subscribe(&IdentTopic::new(t))
                .expect("subscribe works");
        }
        repnet
    }

    pub fn local_peer_id(&self) -> PeerId {
        PeerId::from_public_key(&self.local_key.public())
    }

    fn as_topic(&self, s: &str) -> IdentTopic {
        IdentTopic::new(s)
    }

    pub async fn topics(&self) -> Vec<String> {
        let mut template_names = match self.storage.read().await.list_all_templates().await {
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
        };
        template_names.push("*announcement".to_string());
        template_names
    }

    pub async fn sign_statement(
        &mut self,
        statement: PersistResult<Statement>,
    ) -> Option<SignedStatement> {
        let opinion = UnsignedOpinion {
            date: Date::today(),
            valid: 30,
            serial: 0,
            certainty: 3,
            comment: "".into(),
        };
        let mut storage = self.storage.write().await;
        let own_key = storage.own_key();
        let signed_opinion = opinion.sign_using(&statement.data.signable_bytes(), &own_key.key);
        let signed_opinion = storage
            .persist_opinion(signed_opinion, &statement.id)
            .await
            .unwrap();
        Some(SignedStatement {
            statement: statement.data,
            opinions: vec![signed_opinion.data],
        })
    }

    /// Post a message to a specific peer
    fn post_message(&mut self, peer: &PeerId, message: NetworkMessage) {
        self.rpc.send_request(peer, message);
    }

    /// Publish a message to a topic for all subscribed peers to see
    fn publish_message(&mut self, topic: IdentTopic, message: NetworkMessage) {
        let json = serde_json::to_string(&message).expect("could serialize message");
        match self.gossipsub.publish(topic, json) {
            Ok(mid) => info!("published as {:?}", mid),
            Err(err) => info!("could not publish: {:?}", err),
        };
    }

    pub async fn publish_statement(&mut self, signed_statement: SignedStatement) {
        self.publish_message(
            self.as_topic(&signed_statement.statement.name),
            NetworkMessage::Statement(signed_statement),
        )
    }

    pub async fn announce_infos(&mut self, date: Date) {
        if let Some(infos) = self.sync_state.get_own_infos(date).await {
            self.publish_message(
                self.as_topic("*announcement"),
                NetworkMessage::Announcement(infos.clone()),
            )
        }
    }

    pub async fn handle_network_message(&mut self, event: NetworkMessage, peer: PeerId) {
        // info!("got event: {:?} from {:?}", event, peer);
        match event {
            NetworkMessage::Statement(signed_statement) => {
                info!("got statement: {:?}", signed_statement);
                let statement = signed_statement.statement;
                let mut storage = self.storage.write().await;
                match storage.persist(statement).await {
                    Ok(persist_result) => {
                        info!(
                            "{} statement {} has id {}",
                            persist_result.wording(),
                            persist_result.data,
                            persist_result.id
                        );
                        for signed_opinion in signed_statement.opinions {
                            let result = storage
                                .persist_opinion(signed_opinion, &persist_result.id)
                                .await
                                .expect("could insert opinion");
                            info!(
                                "{} opinion {} has id {}",
                                result.wording(),
                                result.data,
                                result.id
                            );
                        }
                        if persist_result.is_new() && persist_result.name == "template" {
                            if let Entity::Template(template) = &persist_result.entities[0] {
                                self.gossipsub
                                    .subscribe(&self.as_topic(&template.name))
                                    .unwrap();
                            };
                        }
                        self.sync_state.flush_own_infos()
                    }
                    Err(e) => error!("No matching template: {:?}", e),
                }
            }
            NetworkMessage::TemplateRequest => {
                let entities = self
                    .storage
                    .read()
                    .await
                    .list_all_templates()
                    .await
                    .unwrap();
                let key = self.storage.read().await.own_key().key.clone();
                for entity in entities {
                    let statement = Statement {
                        name: "template".into(),
                        entities: vec![entity],
                    };
                    let opinion = UnsignedOpinion::default();
                    let signed_statement = SignedStatement {
                        opinions: vec![opinion.sign_using(&statement.signable_bytes(), &key)],
                        statement: statement,
                    };
                    self.publish_statement(signed_statement).await;
                }
            }
            NetworkMessage::Announcement(infos) => {
                let requested_updates = self.sync_state.add_infos(&peer, &infos).await;
                for t_name in requested_updates {
                    self.post_message(
                        &peer,
                        NetworkMessage::OpinionRequest {
                            name: t_name,
                            date: infos.date,
                        },
                    )
                }
            }
            NetworkMessage::OpinionRequest{name, date} => {
                // answer with all opinions with the given name and start date
                info!("peer {} wants opinions on {} (date: {})", peer, name, date);
                
            }
            _ => {
                error!("Received unhandled message {:?}", event)
            }
        }
    }

    pub async fn handle_behaviour_event(&mut self, event: OutEvent) {
        info!("got behaviour event: {:?}", event);
        match event {
            OutEvent::Mdns(MdnsEvent::Discovered(list)) => {
                let mut peers = HashSet::new();
                for (peer, address) in list {
                    self.rpc.add_address(&peer, address);
                    peers.insert(peer);
                }
                for peer in peers {
                    self.gossipsub.add_explicit_peer(&peer);
                }
            }
            OutEvent::Mdns(MdnsEvent::Expired(list)) => {
                for (peer, _addr) in list {
                    if !self.mdns.has_node(&peer) {
                        self.gossipsub.remove_explicit_peer(&peer);
                    }
                }
            }
            OutEvent::Gossipsub(GossipsubEvent::Message {
                propagation_source: _,
                message_id: _,
                message,
            }) => {
                // only handle messages coming from some peer
                if let Some(peer) = message.source {
                    let string = String::from_utf8_lossy(&message.data);
                    let network_message: NetworkMessage =
                        serde_json::from_str(&string).expect("network message");
                    match self.event_sender.try_send((network_message, peer)) {
                        Err(e) => error!("could not send event: {:?}", e),
                        _ => (),
                    }
                }
            }
            OutEvent::Gossipsub(GossipsubEvent::Subscribed { peer_id, topic }) => {
                if topic.as_str() == "template" {
                    debug!("peer {} wants topics, using broadcast for now", peer_id);
                    self.event_sender
                        .try_send((NetworkMessage::TemplateRequest, peer_id))
                        .unwrap();
                }
            }
            OutEvent::Rpc(RequestResponseEvent::Message { peer, message }) => match message {
                RequestResponseMessage::Request {
                    request_id: _,
                    request,
                    channel,
                } => {
                    match self.event_sender.try_send((request, peer)) {
                        Err(e) => error!("could not send event: {:?}", e),
                        _ => (),
                    }
                    let response = NetworkMessage::None;
                    self.rpc.send_response(channel, response).unwrap()
                }
                _ => (),
            },
            OutEvent::Ping(event) => {
                info!("ping event: {:?}", event);
            }
            _ => (),
        }
    }

    pub async fn handle_connection_established(&mut self, peer_id: PeerId, num_established: u32) {
        info!(
            "got connection to {:?} ({} connections)",
            peer_id, num_established
        );
    }

    pub async fn handle_connection_closed(&mut self, peer_id: PeerId, num_established: u32) {
        info!(
            "connection to {:?} was closed ({} connections)",
            peer_id, num_established
        );
    }
}
