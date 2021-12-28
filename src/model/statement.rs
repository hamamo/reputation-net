use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use nom::combinator::all_consuming;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    entity::Entity,
    parser,
    template::{SpecificTemplate, Template},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub name: String,
    pub entities: Vec<Entity>,
}

impl Statement {
    #[allow(dead_code)]
    pub fn new(name: &str, entities: Vec<Entity>) -> Self {
        Self {
            name: name.into(),
            entities,
        }
    }

    pub fn signer(signer: Entity) -> Self {
        Self::new("signer", vec![signer])
    }

    pub fn specific_template(&self) -> SpecificTemplate {
        SpecificTemplate {
            name: self.name.clone(),
            entity_types: self.entities.iter().map(|x| x.entity_type()).collect(),
        }
    }

    pub fn matches_template(&self, template: &Template) -> bool {
        self.name == template.name && {
            for (entity, entity_type_list) in self.entities.iter().zip(template.entity_types.iter())
            {
                let entity_type = &entity.entity_type();
                if !entity_type_list.contains(entity_type) {
                    return false;
                }
            }
            true
        }
    }

    // create a version of self where literal e-mail addresses are replaced by hashed e-mail addresses
    // hash function is SHA256
    pub fn hash_emails(&self) -> Self {
        Self {
            name: self.name.clone(),
            entities: self.entities.iter().map(|e| e.hash_emails()).collect(),
        }
    }

    /// Return a byte vector for signing
    pub fn signable_bytes(&self) -> Vec<u8> {
        self.to_string().as_bytes().to_vec()
    }
}

impl Display for Statement {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}(", self.name)?;
        let mut first = true;
        for entity in &self.entities {
            if first {
                first = false
            } else {
                write!(f, ",")?;
            }
            write!(f, "{}", entity)?;
        }
        write!(f, ")")
    }
}

impl Serialize for Statement {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Statement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let s: &str = Deserialize::deserialize(deserializer)?;
        match Statement::from_str(s) {
            Ok(e) => Ok(e),
            Err(_) => Err(D::Error::custom("a Statement")),
        }
    }
}

#[derive(Debug)]
pub struct InvalidStatement;
impl fmt::Display for InvalidStatement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid statement format")
    }
}

impl FromStr for Statement {
    type Err = InvalidStatement;
    fn from_str(s: &str) -> Result<Self, InvalidStatement> {
        match all_consuming(parser::statement)(s) {
            Ok((_, stmt)) => Ok(stmt),
            _ => Err(InvalidStatement),
        }
    }
}

impl From<&str> for Statement {
    fn from(s: &str) -> Self {
        match all_consuming(parser::statement)(s) {
            Ok((_, stmt)) => stmt,
            _ => panic!("expected a statement"),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::str::FromStr;

    pub fn example() -> Statement {
        Statement {
            name: "abuse".into(),
            entities: vec![
                Entity::Domain("example.com".into()),
                Entity::EMail("abuse@example.com".into()),
            ],
        }
    }

    #[test]
    fn print() {
        let stmt_src = "abuse(example.com,abuse@example.com)";
        let stmt = example();
        assert_eq!(stmt_src, stmt.to_string());
    }

    #[test]
    fn template_statement() {
        let stmt_src = "template(exploited_host(Domain|IPv4))";
        let stmt = Statement::from_str(stmt_src).unwrap();
        assert_eq!(stmt_src, stmt.to_string());
    }

    #[test]
    fn match_template() {
        let stmt = Statement::from_str("abuse(example.com,abuse@example.com)").unwrap();
        let template = Template::from_str("abuse(Domain,EMail|Url)").unwrap();
        assert!(stmt.matches_template(&template));
    }
}
