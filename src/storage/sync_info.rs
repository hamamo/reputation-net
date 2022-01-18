use std::collections::HashMap;

use libp2p::multihash::{Sha2_256, StatefulHasher};
use serde::{Deserialize, Serialize};

use crate::model::Date;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncInfo {
    count: usize,
    hash: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncInfos {
    pub date: Date,
    pub infos: HashMap<String, SyncInfo>,
}

impl SyncInfo {
    pub fn new(data: Vec<Vec<u8>>) -> Self {
        let count = data.len();
        let mut hasher = Sha2_256::default();
        for s in data {
            hasher.update(&s)
        }
        let hash = base64::encode(hasher.finalize());
        Self { count, hash }
    }

    /// Does the info in `other` suggest that we should update our data?
    pub fn suggests_update(&self, other: &Self) -> bool {
        self.count < other.count || (self.count == other.count && self.hash != other.hash)
    }
}
