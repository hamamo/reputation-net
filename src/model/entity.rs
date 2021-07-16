use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str::FromStr;

use cidr::{Ipv4Cidr, Ipv6Cidr};
use libp2p::multihash::{Hasher, Sha2_256};

use super::{template::Template, PublicKey};

// EntityType is used within Templates
#[derive(Clone, PartialEq, Debug)]
pub enum EntityType {
    Template = 1,
    Signer = 2,
    Domain = 3,
    EMail = 4,
    HashedEMail = 5,
    AS = 6,
    IPv4 = 7,
    IPv6 = 8,
    Url = 9,
}

impl Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Domain => "Domain",
                Self::EMail => "EMail",
                Self::AS => "AS",
                Self::IPv4 => "IPv4",
                Self::IPv6 => "IPv6",
                Self::Signer => "Signer",
                Self::Url => "Url",
                Self::HashedEMail => "HashedEMail",
                Self::Template => "Template",
            }
        )
    }
}

#[derive(Debug)]
pub struct InvalidEntityType;
impl Display for InvalidEntityType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid entity type")
    }
}

impl FromStr for EntityType {
    type Err = InvalidEntityType;
    fn from_str(string: &str) -> Result<Self, InvalidEntityType> {
        let x = match string {
            "Domain" => Self::Domain,
            "EMail" => Self::EMail,
            "AS" => Self::AS,
            "IPv4" => Self::IPv4,
            "IPv6" => Self::IPv6,
            "Signer" => Self::Signer,
            "HashedEMail" => Self::HashedEMail,
            "Template" => Self::Template,
            _ => return Err(InvalidEntityType),
        };
        Ok(x)
    }
}

impl EntityType {
    pub fn vec_from_str(string: &str) -> Result<Vec<Self>, InvalidEntityType> {
        string.split("|").map(|x| Self::from_str(x)).collect()
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Entity {
    Domain(String),      // denotes a domain name
    EMail(String),       // denotes an e-mail address (localpart@domain)
    AS(u32),             // denotes an autonomous system
    IPv4(Ipv4Cidr),      // denotes an IPv4 address or address range
    IPv6(Ipv6Cidr),      // denotes an IPv4 address or address range
    Signer(PublicKey),   // denotes a signer
    Url(String),         // denotes an URL, for example a contact form
    HashedEMail(String), // hash of an e-mail, to protect personal data
    Template(Template),  // statement template to dynamically add new statement types
}

// an error type
#[derive(Debug)]
pub struct InvalidEntity;
impl Display for InvalidEntity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid entity format")
    }
}

impl Entity {
    #[allow(dead_code)]
    pub fn hashed_email_for(email: &str) -> Self {
        let digest = Sha2_256::digest(email.as_bytes());
        Self::HashedEMail(base64::encode(digest))
    }

    pub fn entity_type(&self) -> EntityType {
        match self {
            Self::Domain(_) => EntityType::Domain,
            Self::EMail(_) => EntityType::EMail,
            Self::AS(_) => EntityType::AS,
            Self::IPv4(_) => EntityType::IPv4,
            Self::IPv6(_) => EntityType::IPv6,
            Self::Signer(_) => EntityType::Signer,
            Self::Url(_) => EntityType::Url,
            Self::HashedEMail(_) => EntityType::HashedEMail,
            Self::Template(_) => EntityType::Template,
        }
    }
}

impl Display for Entity {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Entity::Domain(s) => write!(f, "{}", s),
            Entity::EMail(s) => write!(f, "{}", s),
            Entity::AS(n) => write!(f, "AS{}", n),
            Entity::IPv4(ip) => write!(f, "{}", ip),
            Entity::IPv6(ip) => write!(f, "{}", ip),
            Entity::Signer(pk) => write!(f, "{}", pk),
            Entity::Url(s) => write!(f, "{}", s),
            Entity::HashedEMail(s) => write!(f, "#@{}", s),
            Entity::Template(t) => write!(f, "{}", t),
        }
    }
}

impl FromStr for Entity {
    type Err = InvalidEntity;
    fn from_str(string: &str) -> Result<Self, InvalidEntity> {
        match nom::combinator::all_consuming(super::parser::entity)(string) {
            Ok((_, entity)) => Ok(entity),
            _ => Err(InvalidEntity {}),
        }
    }
}

mod tests {

    use std::str::FromStr;

    use cidr::Ipv4Cidr;

    use super::{Entity, EntityType};

    #[test]
    fn domain() {
        const INPUT: &str = "example.com";
        let entity: Entity = INPUT.parse().unwrap();
        assert_eq!(entity, Entity::Domain("example.com".into()));
        assert_eq!(entity.to_string(), INPUT);
    }
    #[test]
    fn email() {
        const INPUT: &str = "user@example.com";
        let entity: Entity = INPUT.parse().unwrap();
        assert_eq!(entity, Entity::EMail("user@example.com".into()));
        assert_eq!(entity.to_string(), INPUT);
    }
    #[test]
    fn asn() {
        const INPUT: &str = "AS12345";
        let entity: Entity = INPUT.parse().unwrap();
        assert_eq!(entity, Entity::AS(12345));
        assert_eq!(entity.to_string(), INPUT);
    }
    #[test]
    fn cidr() {
        const INPUT: &str = "168.168.168.168";
        let entity: Entity = INPUT.parse().unwrap();
        assert_eq!(
            entity,
            Entity::IPv4(Ipv4Cidr::from_str("168.168.168.168").unwrap())
        );
        assert_eq!(entity.to_string(), INPUT);
    }
    #[test]
    fn types() {
        for type_str in vec![
            "Domain",
            "EMail",
            "AS",
            "IPv4",
            "IPv6",
            "Signer",
            "HashedEMail",
            "Template",
        ] {
            assert_eq!(
                type_str,
                format!("{}", EntityType::from_str(type_str).unwrap())
            )
        }
    }
}
