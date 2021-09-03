use libp2p::identity::{Keypair};

use super::{Entity, PublicKey};

pub struct Trust {
    pub signer: Entity,
    pub level: u8,
    pub key: Option<Keypair>,
}

impl Trust {

    // create a new (owner) Trust entry
    pub fn new() -> Self {
        let keypair = Keypair::generate_secp256k1();
        let signer = Entity::Signer(PublicKey{key: keypair.public()});
        Self {
            signer: signer,
            level: 0,
            key: Some(keypair)
        }
    }

    pub fn privkey_string(&self) -> String {
        match &self.key {
            Some(Keypair::Secp256k1(keypair)) => base64::encode(&keypair.secret().to_bytes()),
            _ => "".to_string()
        }
    }
}