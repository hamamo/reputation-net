use std::{collections::HashSet, sync::Arc, time::Duration};

use tokio::sync::RwLock;

use futures::channel::mpsc::Sender;
use log::{error, info};

#[cfg(feature = "autonat")]
use libp2p::autonat;

use libp2p::{
    gossipsub::{
        Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity, TopicHash,
    },
    identify::{Identify, IdentifyConfig, IdentifyEvent},
    identity::Keypair,
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    ping::{Ping, PingConfig, PingEvent},
    request_response::{
        ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
        RequestResponseMessage, ResponseChannel,
    },
    NetworkBehaviour, PeerId,
};

use crate::storage::Persistent;

use super::{
    model::{Date, Entity, SignedStatement, Statement, UnsignedOpinion},
    storage::{Persist, Storage},
};

mod messages;
pub use messages::*;

mod rpc;
use rpc::*;

mod sync;
use sync::*;

mod user_input;
pub use user_input::input_reader;

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent")]
pub struct ReputationNet {
    identify: Identify,
    #[cfg(feature = "autonat")]
    autonat: autonat::Behaviour,
    mdns: Mdns,
    gossipsub: Gossipsub,
    ping: Ping,
    rpc: RequestResponse<RpcCodec>,

    #[behaviour(ignore)]
    pub storage: Arc<RwLock<Storage>>,
    #[behaviour(ignore)]
    event_sender: Sender<Message>,
    #[behaviour(ignore)]
    pub local_key: Keypair,
    #[behaviour(ignore)]
    sync_state: SyncState,
}

#[derive(Debug)]
pub enum OutEvent {
    Identify(IdentifyEvent),
    #[cfg(feature = "autonat")]
    Autonat(autonat::Event),
    Mdns(MdnsEvent),
    Gossipsub(GossipsubEvent),
    Ping(PingEvent),
    Rpc(RequestResponseEvent<RpcRequest, RpcResponse>),
}

impl From<IdentifyEvent> for OutEvent {
    fn from(v: IdentifyEvent) -> Self {
        Self::Identify(v)
    }
}

#[cfg(feature = "autonat")]
impl From<autonat::Event> for OutEvent {
    fn from(v: autonat::Event) -> Self {
        Self::Autonat(v)
    }
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

impl From<RequestResponseEvent<RpcRequest, RpcResponse>> for OutEvent {
    fn from(v: RequestResponseEvent<RpcRequest, RpcResponse>) -> Self {
        Self::Rpc(v)
    }
}

impl ReputationNet {
    pub async fn new(message_sender: Sender<Message>) -> Self {
        let storage = Storage::new().await;
        let keypair = storage.own_key().key.clone();
        #[cfg(feature = "autonat")]
        let local_peer_id = PeerId::from_public_key(&keypair.public());
        let storage = Arc::new(RwLock::new(storage));
        let mut repnet = Self {
            #[cfg(feature = "autonat")]
            autonat: autonat::Behaviour::new(
                local_peer_id,
                autonat::Config {
                    // use_only_global_ips: true,
                    ..autonat::Config::default()
                },
            ),
            identify: Identify::new(IdentifyConfig::new(
                "reputation-net/0.1.0".to_string(),
                keypair.public(),
            )),
            gossipsub: Gossipsub::new(
                MessageAuthenticity::Signed(keypair.clone()),
                GossipsubConfig::default(),
            )
            .unwrap(),
            mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
            ping: Ping::new(
                PingConfig::new()
                    .with_interval(Duration::new(300, 0))
                    .with_keep_alive(true),
            ),
            rpc: RequestResponse::new(
                RpcCodec {},
                vec![(RpcProtocol::Version1, ProtocolSupport::Full)].into_iter(),
                RequestResponseConfig::default(),
            ),
            storage: storage.clone(),
            event_sender: message_sender,
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
        let mut topics = match self.storage.read().await.list_all_templates().await {
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
        topics.push("*announcement".to_string());
        topics
    }

    pub async fn sign_statement(
        &mut self,
        statement: Persistent<Statement>,
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
            .unwrap()
            .data;
        let _ = storage.update_last_used(vec![statement.id]).await;
        Some(SignedStatement {
            statement: statement.data,
            opinions: vec![signed_opinion.data],
        })
    }

    /// Post a message to a specific peer
    fn post_message(&mut self, peer: &PeerId, request: RpcRequest) {
        self.rpc.send_request(peer, request);
    }

    /// Publish a message to a topic for all subscribed peers to see
    fn publish_message(&mut self, topic: IdentTopic, message: BroadcastMessage) {
        let json = serde_json::to_string(&message).expect("could serialize message");
        match self.gossipsub.publish(topic, json) {
            Ok(mid) => info!("published as {:?}", mid),
            Err(err) => info!("could not publish: {:?}", err),
        };
    }

    pub fn publish_statement(&mut self, signed_statement: SignedStatement) {
        self.publish_message(
            self.as_topic(&signed_statement.statement.name),
            BroadcastMessage::Statement(signed_statement),
        )
    }

    pub async fn announce_infos(&mut self, date: Date) {
        if let Some(infos) = self.sync_state.get_own_infos(date).await {
            self.publish_message(
                self.as_topic("*announcement"),
                BroadcastMessage::Announcement(infos.clone()),
            )
        }
    }

    pub async fn handle_message(&mut self, message: Message) {
        match message {
            Message::Broadcast {
                message,
                peer_id,
                topic,
            } => self.handle_broadcast_message(message, peer_id, topic).await,
            Message::Request {
                request,
                peer_id,
                response_channel,
            } => {
                self.handle_request_message(request, peer_id, response_channel)
                    .await
            }
            Message::Response { peer_id, response } => {
                self.handle_response_message(response, peer_id).await
            }
            Message::SendAnnouncement { peer_id } => {
                if let Some(infos) = self.sync_state.get_own_infos(Date::today()).await {
                    self.post_message(&peer_id, RpcRequest::Announcement(infos));
                }
                if let Some(infos) = self.sync_state.get_own_infos(Date::yesterday()).await {
                    self.post_message(&peer_id, RpcRequest::Announcement(infos));
                }
            }
        }
    }

    pub async fn handle_broadcast_message(
        &mut self,
        message: BroadcastMessage,
        peer_id: PeerId,
        _topic: TopicHash,
    ) {
        match message {
            BroadcastMessage::Statement(signed_statement) => {
                let statement = signed_statement.statement;
                let mut storage = self.storage.write().await;
                match storage.persist(statement).await {
                    Ok(persist_result) => {
                        info!("{}", persist_result);
                        let persistent_statement = &persist_result.data;
                        for signed_opinion in signed_statement.opinions {
                            let result = storage
                                .persist_opinion(signed_opinion, &persistent_statement.id)
                                .await
                                .expect("could insert opinion");
                            info!("{}", result);
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
            BroadcastMessage::Announcement(infos) => {
                let requested_updates = self.sync_state.add_infos(&peer_id, &infos).await;
                for t_name in requested_updates {
                    self.post_message(
                        &peer_id,
                        RpcRequest::OpinionRequest {
                            name: t_name,
                            date: infos.date,
                        },
                    )
                }
            }
        }
    }

    pub async fn handle_request_message(
        &mut self,
        request: RpcRequest,
        peer_id: PeerId,
        response_channel: ResponseChannel<RpcResponse>,
    ) {
        // println!("got request message {:?} from {}", request, peer_id);
        let response = match request {
            RpcRequest::OpinionRequest { name, date } => {
                let storage = self.storage.read().await;
                match storage.list_statements_named_signed(&name, date).await {
                    Ok(list) => RpcResponse::Statements(list),
                    Err(e) => {
                        error!("{:?}", e);
                        RpcResponse::None
                    }
                }
            }
            RpcRequest::TemplateRequest => {
                let entities = self
                    .storage
                    .read()
                    .await
                    .list_all_templates()
                    .await
                    .unwrap();
                let key = self.storage.read().await.own_key().key.clone();
                let mut statements = vec![];
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
                    statements.push(signed_statement);
                }
                RpcResponse::Statements(statements)
            }
            RpcRequest::Announcement(infos) => {
                let requested_updates = self.sync_state.add_infos(&peer_id, &infos).await;
                for t_name in requested_updates {
                    self.post_message(
                        &peer_id,
                        RpcRequest::OpinionRequest {
                            name: t_name,
                            date: infos.date,
                        },
                    )
                }
                RpcResponse::None
            }
        };
        // println!("sending response {:?}", response);
        self.rpc.send_response(response_channel, response).unwrap();
    }

    pub async fn handle_response_message(&mut self, response: RpcResponse, _peer_id: PeerId) {
        // println!("got response message {:?} from {}", response, peer_id);
        match response {
            RpcResponse::Statements(list) => {
                for signed_statement in list {
                    info!("got statement in response: {}", signed_statement.statement);
                    let mut storage = self.storage.write().await;
                    let persistent_statement = storage
                        .persist(signed_statement.statement)
                        .await
                        .expect("could persist statement")
                        .data;
                    for opinion in signed_statement.opinions {
                        storage
                            .persist_opinion(opinion, &persistent_statement.id)
                            .await
                            .expect("could persist opinion");
                    }
                }
                self.sync_state.flush_own_infos()
            }
            RpcResponse::None => (),
        }
    }

    pub fn handle_behaviour_event(&mut self, event: OutEvent) {
        info!("got behaviour event: {:?}", event);
        match event {
            OutEvent::Identify(event) => self.handle_identify_event(event),
            #[cfg(feature = "autonat")]
            OutEvent::Autonat(event) => {
                if let autonat::Event::StatusChanged { old, new } = event {
                    println!("autonat: status {:?} -> {:?}", old, new);
                }
            }
            OutEvent::Ping(event) => {
                info!("ping event: {:?}", event);
            }
            OutEvent::Mdns(event) => self.handle_mdns_event(event),
            OutEvent::Gossipsub(event) => self.handle_gossipsub_event(event),
            OutEvent::Rpc(event) => self.handle_rpc_event(event),
        }
    }

    pub fn handle_identify_event(&mut self, event: IdentifyEvent) {
        match event {
            IdentifyEvent::Received { peer_id, info } => {
                for address in info.listen_addrs {
                    self.rpc.add_address(&peer_id, address)
                }
                self.gossipsub.add_explicit_peer(&peer_id);
            }
            _ => (),
        }
    }

    fn handle_mdns_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                let mut peers = HashSet::new();
                for (peer, address) in list {
                    self.rpc.add_address(&peer, address);
                    peers.insert(peer);
                }
                for peer in peers {
                    self.gossipsub.add_explicit_peer(&peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _addr) in list {
                    if !self.mdns.has_node(&peer) {
                        // self.gossipsub.remove_explicit_peer(&peer);
                    }
                }
            }
        }
    }

    fn handle_gossipsub_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message {
                propagation_source: _,
                message_id: _,
                message,
            } => {
                // only handle messages coming from some peer
                if let Some(peer) = message.source {
                    let string = String::from_utf8_lossy(&message.data);
                    let message = Message::Broadcast {
                        message: serde_json::from_str(&string).expect("network message"),
                        peer_id: peer,
                        topic: message.topic,
                    };
                    if let Err(e) = self.event_sender.try_send(message) {
                        error!("could not send event: {:?}", e)
                    }
                }
            }
            GossipsubEvent::Subscribed {
                peer_id: _,
                topic: _,
            } => (),
            GossipsubEvent::Unsubscribed {
                peer_id: _,
                topic: _,
            } => (),
            GossipsubEvent::GossipsubNotSupported { peer_id } => {
                println!("gossipsub not supported: {}", peer_id);
            }
        }
    }

    fn handle_rpc_event(&mut self, event: RequestResponseEvent<RpcRequest, RpcResponse>) {
        match event {
            RequestResponseEvent::Message { peer, message } => match message {
                RequestResponseMessage::Request {
                    request_id: _,
                    request,
                    channel,
                } => {
                    let message = Message::Request {
                        request,
                        peer_id: peer,
                        response_channel: channel,
                    };
                    if let Err(e) = self.event_sender.try_send(message) {
                        error!("RPC could not send request: {:?}", e)
                    }
                }
                RequestResponseMessage::Response {
                    request_id: _,
                    response,
                } => {
                    let message = Message::Response {
                        response,
                        peer_id: peer,
                    };
                    if let Err(e) = self.event_sender.try_send(message) {
                        error!("RPC could not send response: {:?}", e)
                    }
                }
            },
            RequestResponseEvent::OutboundFailure {
                peer,
                request_id,
                error,
            } => {
                println!("RPC outbound failure: {} {} ({})", peer, request_id, error)
            }
            RequestResponseEvent::InboundFailure {
                peer,
                request_id,
                error,
            } => {
                println!("RPC inbound failure: {} {} ({})", peer, request_id, error)
            }
            RequestResponseEvent::ResponseSent {
                peer: _,
                request_id: _,
            } => {
                // println!("response sent: {} {}", peer, request_id)
            }
        }
    }

    pub fn handle_connection_established(
        &mut self,
        peer_id: PeerId,
        num_connections_with_peer: usize,
        num_peers: usize,
    ) {
        println!(
            "got connection with {:?} ({} connections, {} peers)",
            peer_id, num_connections_with_peer, num_peers
        );
        self.post_message(&peer_id, RpcRequest::TemplateRequest);
        if num_connections_with_peer == 1 {
            // first connection to that peer
            let message = Message::SendAnnouncement { peer_id };
            if let Err(e) = self.event_sender.try_send(message) {
                error!("could not send event: {:?}", e)
            };
        }
    }

    pub fn handle_connection_closed(
        &mut self,
        peer_id: PeerId,
        num_connections_with_peer: usize,
        num_peers: usize,
    ) {
        println!(
            "connection with {:?} was closed ({} connections, {} peers)",
            peer_id, num_connections_with_peer, num_peers
        );
        if num_connections_with_peer == 0 {
            // try to reconnect
        }
    }
}
