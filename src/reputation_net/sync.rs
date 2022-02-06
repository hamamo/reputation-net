use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use libp2p::PeerId;
use log::info;

use crate::{
    model::Date,
    storage::{Storage, SyncInfos},
};

// Synchronization support (basically allowing nodes to fill their database on startup)

/// A node's own guess about its synchronization state
pub struct SyncState {
    own_infos: HashMap<Date, SyncInfos>,
    storage: Arc<RwLock<Storage>>,
}

impl SyncState {
    pub async fn new(storage: Arc<RwLock<Storage>>) -> Self {
        Self {
            own_infos: HashMap::new(),
            storage,
        }
    }

    /// Update our own infos for a given date. Return infos if successful
    pub async fn update_own_infos(&mut self, date: &Date) -> Option<SyncInfos> {
        let storage = self.storage.read().await;
        match storage.get_sync_infos(*date).await {
            Ok(infos) => {
                self.own_infos.insert(infos.date, infos.clone());
                Some(infos)
            }
            _ => None,
        }
    }

    /// return the template names for which we want data from this peer
    pub async fn add_infos(&mut self, peer: &PeerId, infos: &SyncInfos) -> Vec<String> {
        info!("adding info {:?} for peer {:?}", infos, peer);
        let mut template_names = vec![];
        if let Some(own_infos) = self.get_own_infos(infos.date).await {
            for (key, info) in infos.infos.iter() {
                if let Some(own_info) = own_infos.infos.get(key) {
                    if own_info.suggests_update(&info) {
                        template_names.push(key.clone());
                    }
                } else {
                    template_names.push(key.clone());
                }
            }
        }
        info!("need updates for template names {:?}", template_names);
        template_names
    }

    pub async fn get_own_infos(&mut self, date: Date) -> Option<SyncInfos> {
        match self.own_infos.get(&date) {
            Some(infos) => Some(infos.clone()),
            None => self.update_own_infos(&date).await,
        }
    }

    pub fn flush_own_infos(&mut self) {
        self.own_infos = HashMap::new()
    }
}
