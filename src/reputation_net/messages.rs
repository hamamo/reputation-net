use serde::{Deserialize, Serialize};

use crate::model::{Date, SignedStatement};

use crate::storage::SyncInfos;

/// A number of responses can be sent in response to gossipsub requests, so technically they are sent as requests.
/// It's easiest to just keep both in one type.

#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkMessage {
    None,
    TemplateRequest,
    Announcement(SyncInfos),
    OpinionRequest { name: String, date: Date },
    Statement(SignedStatement),
}
