use libp2p::gossipsub::TopicHash;
use libp2p::{request_response::ResponseChannel, PeerId};
use serde::{Deserialize, Serialize};

use crate::model::{Date, SignedStatement};

use crate::storage::SyncInfos;

/// A number of responses can be sent in response to gossipsub requests, so technically they are sent as requests.
/// It's easiest to just keep both in one type.

#[derive(Debug, Serialize, Deserialize)]
pub enum BroadcastMessage {
    TemplateRequest,
    Announcement(SyncInfos),
    OpinionRequest { name: String, date: Date },
    Statement(SignedStatement),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RpcRequest {
    OpinionRequest { name: String, date: Date },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RpcResponse {
    None,
    Statements(Vec<SignedStatement>),
}

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
}
