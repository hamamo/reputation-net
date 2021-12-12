use libp2p::identity::{Keypair};

use super::{Entity, PublicKey};

pub struct OwnKey {
    pub signer: Entity,
    pub level: u8,
    pub key: Keypair,
}

impl OwnKey {

    // create a new (owner) Trust entry
    pub fn new() -> Self {
        let keypair = Keypair::generate_secp256k1();
        let signer = Entity::Signer(PublicKey{key: keypair.public()});
        Self {
            signer: signer,
            level: 0,
            key: keypair
        }
    }

    pub fn privkey_string(&self) -> String {
        match &self.key {
            Keypair::Secp256k1(keypair) => base64::encode(&keypair.secret().to_bytes()),
            _ => "".into()
        }
    }
}