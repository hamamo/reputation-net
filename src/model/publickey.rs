use std::{
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
    str::FromStr,
};

use serde::{de::Visitor, Deserialize, Serialize, Serializer};

pub type Signature = Vec<u8>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    pub key: libp2p::core::PublicKey,
}

#[derive(Debug)]
pub struct InvalidPublicKey;

struct PublicKeyVisitor;

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

impl Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_string().hash(state);
    }
}

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

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Visitor<'de> for PublicKeyVisitor {
    type Value = PublicKey;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a public key string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match Self::Value::from_str(v) {
            Ok(public_key) => Ok(public_key),
            Err(e) => Err(E::custom(e.to_string())),
        }
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(PublicKeyVisitor)
    }
}
