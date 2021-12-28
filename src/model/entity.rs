use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str::FromStr;

use cidr::{Ipv4Cidr, Ipv6Cidr};
use libp2p::multihash::{Hasher, Sha2_256};
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use super::{template::Template, PublicKey};

// EntityType is used within Templates
#[derive(Copy, Clone, PartialEq, Debug, Eq, Hash)]
pub enum EntityType {
    Template = 1,
    Signer = 2,
    Domain = 3,
    EMail = 4,
    HashValue = 5,
    AS = 6,
    IPv4 = 7,
    IPv6 = 8,
    Url = 9,
}

#[derive(Debug)]
pub struct InvalidEntityType;

#[derive(Debug)]
pub struct InvalidEntity;

#[derive(Clone, PartialEq, Debug)]
pub enum Entity {
    Domain(String),    // denotes a domain name
    EMail(String),     // denotes an e-mail address (localpart@domain)
    AS(u32),           // denotes an autonomous system
    IPv4(Ipv4Cidr),    // denotes an IPv4 address or address range
    IPv6(Ipv6Cidr),    // denotes an IPv4 address or address range
    Signer(PublicKey), // denotes a signer
    #[allow(dead_code)]
    Url(String), // denotes an URL, for example a contact form
    HashValue(String), // hash of an e-mail or other data. may be used to cloak user data, or to secure URL contents
    Template(Template), // statement template to dynamically add new statement types
}

impl Display for InvalidEntityType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid entity type")
    }
}
impl Display for InvalidEntity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid entity format")
    }
}

impl std::error::Error for InvalidEntity {}

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
                Self::HashValue => "HashValue",
                Self::Template => "Template",
            }
        )
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
            "HashValue" => Self::HashValue,
            "Template" => Self::Template,
            _ => return Err(InvalidEntityType),
        };
        Ok(x)
    }
}

impl Entity {
    #[allow(dead_code)]
    pub fn hash_string(string: &str) -> Self {
        let digest = Sha2_256::digest(string.as_bytes());
        Self::HashValue(base64::encode(digest))
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
            Self::HashValue(_) => EntityType::HashValue,
            Self::Template(_) => EntityType::Template,
        }
    }

    pub fn hash_emails(&self) -> Self {
        match self {
            Self::EMail(x) => Self::hash_string(x),
            _ => self.clone(),
        }
    }

    pub fn domain(&self) -> Option<Self> {
        match self {
            Self::EMail(address) => {
                let at_index = address.find("@").unwrap();
                Some(Self::Domain(address[at_index + 1..].into()))
            }
            Self::Domain(domain) => {
                let dot_index = domain.find(".");
                match dot_index {
                    None => None,
                    Some(n) => {
                        if n == domain.len() - 1 {
                            None
                        } else {
                            let mut super_domain = domain[n + 1..].to_string();
                            if let None = super_domain.find(".") {
                                super_domain.push_str(".");
                            }
                            Some(Self::Domain(super_domain))
                        }
                    }
                }
            }
            _ => None,
        }
    }

    /// Return a list of all lookup keys that should be considered to find matching statements, from most to least specific
    pub fn all_lookup_keys(&self) -> Vec<Self> {
        match self {
            Self::EMail(_) => {
                let mut result = vec![self.clone(), self.hash_emails()];
                let mut domains = self.domain().unwrap().all_lookup_keys();
                result.append(&mut domains);
                result
            }
            Self::Domain(_) => {
                let mut result = vec![self.clone()];
                if let Some(super_domain) = self.domain() {
                    result.append(&mut super_domain.all_lookup_keys())
                }
                result
            }
            _ => vec![self.clone()],
        }
    }

    /// Return a pair of cidr_min and cidr_max strings for database indexing
    pub fn cidr_minmax(&self) -> (Option<String>, Option<String>) {
        match self {
            Entity::IPv4(cidr) => {
                let min = cidr.first_address().octets();
                let max = cidr.last_address().octets();
                (
                    Some(format!(
                        "{:02X}{:02X}{:02X}{:02X}",
                        min[0], min[1], min[2], min[3]
                    )),
                    Some(format!(
                        "{:02X}{:02X}{:02X}{:02X}",
                        max[0], max[1], max[2], max[3]
                    )),
                )
            }
            _ => (None, None),
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
            Entity::HashValue(s) => write!(f, "#{}", s),
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

impl Serialize for Entity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Entity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        match Entity::from_str(s) {
            Ok(e) => Ok(e),
            Err(_) => Err(D::Error::custom("an Entity")),
        }
    }
}

#[cfg(test)]
mod tests {

    use std::str::FromStr;

    use cidr::Ipv4Cidr;

    use super::*;

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
            "HashValue",
            "Template",
        ] {
            assert_eq!(
                type_str,
                format!("{}", EntityType::from_str(type_str).unwrap())
            )
        }
    }
    #[test]
    fn signer_display() {
        let keypair = super::super::tests::example_keypair();
        let pk = keypair.public();
        let signer = Entity::Signer(super::super::publickey::PublicKey { key: pk });
        assert_eq!(signer.to_string(), super::super::tests::example_signer());
    }
    #[test]
    fn domain_lookup_keys() {
        let domain = Entity::Domain("domain.example.biz".into());
        assert_eq!(
            domain.all_lookup_keys(),
            vec![
                domain.clone(),
                Entity::Domain("example.biz".into()),
                Entity::Domain("biz.".into())
            ]
        )
    }
    #[test]
    fn email_lookup_keys() {
        let email = Entity::EMail("spammer@example.com".into());
        assert_eq!(
            email.all_lookup_keys(),
            vec![
                email.clone(),
                email.hash_emails(),
                Entity::Domain("example.com".into()),
                Entity::Domain("com.".into())
            ]
        )
    }
}
