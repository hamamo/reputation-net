use serde::{Deserialize, Serialize};

use crate::model::{SignedStatement, Date};

/// A number of responses can be sent in response to gossipsub requests, so technically they are sent as requests.
/// It's easiest to just keep both in one type.

#[derive(Debug,Serialize,Deserialize)]
pub enum NetworkMessage {
    None,
    TemplateRequest,
    Announcement(Date, Vec<Announcement>),
    OpinionRequest(OpinionRequest),
    Statement(SignedStatement),
}

#[derive(Debug,Serialize,Deserialize)]
pub struct Announcement {
    name: String,
    count: u32,
    hash: String,
}

#[derive(Debug,Serialize,Deserialize)]
pub struct OpinionRequest {
    name: String,
    date: Date
}