use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

pub type Signature = Vec<u8>;

#[derive(Clone, Debug, PartialEq)]
pub struct PublicKey {
    pub key: libp2p::core::PublicKey,
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match &self.key {
            libp2p::core::PublicKey::Ed25519(pk) => {
                write!(f, "ed25519:{}", &base64::encode(pk.encode()))
            }
            libp2p::core::PublicKey::Secp256k1(pk) => {
                write!(f, "secp256k1:{}", &base64::encode(pk.encode()))
            }
            libp2p::core::PublicKey::Rsa(pk) => {
                write!(f, "rsa:{}", &base64::encode(pk.encode_x509()))
            }
        }
    }
}

#[derive(Debug)]
pub struct InvalidPublicKey;
impl fmt::Display for InvalidPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid public key format")
    }
}

impl FromStr for PublicKey {
    type Err = InvalidPublicKey;
    fn from_str(s: &str) -> Result<Self, InvalidPublicKey> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() < 2 {
            return Err(InvalidPublicKey);
        }
        let bytes = if let Ok(data) = base64::decode(parts[1]) {
            data
        } else {
            return Err(InvalidPublicKey);
        };
        match parts[0] {
            "ed25519" => Ok(Self {
                key: libp2p::core::PublicKey::Ed25519(
                    if let Ok(data) = libp2p::identity::ed25519::PublicKey::decode(&bytes) {
                        data
                    } else {
                        return Err(InvalidPublicKey);
                    },
                ),
            }),
            "secp256k1" => Ok(Self {
                key: libp2p::core::PublicKey::Secp256k1(
                    if let Ok(data) = libp2p::identity::secp256k1::PublicKey::decode(&bytes) {
                        data
                    } else {
                        return Err(InvalidPublicKey);
                    },
                ),
            }),
            "rsa" => Ok(Self {
                key: libp2p::core::PublicKey::Rsa(
                    if let Ok(data) = libp2p::identity::rsa::PublicKey::decode_x509(&bytes) {
                        data
                    } else {
                        return Err(InvalidPublicKey);
                    },
                ),
            }),
            _ => Err(InvalidPublicKey),
        }
    }
}