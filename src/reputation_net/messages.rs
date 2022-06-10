use libp2p::gossipsub::TopicHash;
use libp2p::{request_response::ResponseChannel, PeerId};
use serde::{Deserialize, Serialize};

use crate::model::{Date, SignedStatement};

use crate::storage::SyncInfos;

/// Broadcast messages are sent using GossipSub to all peers in the network
#[derive(Debug, Serialize, Deserialize)]
pub enum BroadcastMessage {
    Announcement(SyncInfos),
    Statement(SignedStatement),
}

/// Rpc requests are sent to a specific peer in reaction to some event (announcement or connection establishment)
#[derive(Debug, Serialize, Deserialize)]
pub enum RpcRequest {
    TemplateRequest,
    Announcement(SyncInfos),
    OpinionRequest { name: String, date: Date },
}

/// Rpc responses are only sent in response to rpc requests
#[derive(Debug, Serialize, Deserialize)]
pub enum RpcResponse {
    None,
    Statements(Vec<SignedStatement>),
}

/// This enum is used to communicate broadcast and rpc messages from the receiving NetworkBehaviour to the central dispatch
#[derive(Debug)]
pub enum Message {
    Broadcast {
        peer_id: PeerId,
        message: BroadcastMessage,
        topic: TopicHash
    },
    Request {
        peer_id: PeerId,
        request: RpcRequest,
        response_channel: ResponseChannel<RpcResponse>,
    },
    Response {
        peer_id: PeerId,
        response: RpcResponse,
    },
    SendAnnouncement {
        peer_id: PeerId,
    }
}
